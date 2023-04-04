use crate::actually_private::Private;
use crate::UniquePtr;
// #[cfg(feature = "alloc")]
// use alloc::borrow::Cow;
#[cfg(feature = "alloc")]
use alloc::string::String;
use core::cmp::Ordering;
use core::fmt::{self, Debug, Display};
use core::hash::{Hash, Hasher};
use core::marker::{PhantomData, PhantomPinned};
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::slice;
use core::str::{self};

use widestring::{U32CStr, U32CString, Utf32Str, Utf32String};

/// In C++, wchar_t may be signed or unsigned, but is in practice signed.
/// In Rust UTF32String, its wchar_t is unsigned.
/// Use unsigned to ease interop.
type wchar_t = u32;

extern "C" {
    #[link_name = "cxxbridge1$cxx_wstring$init"]
    fn wstring_init(this: &mut MaybeUninit<CxxWString>, ptr: *const wchar_t, len: usize);
    #[link_name = "cxxbridge1$cxx_wstring$destroy"]
    fn wstring_destroy(this: &mut MaybeUninit<CxxWString>);
    #[link_name = "cxxbridge1$cxx_wstring$data"]
    fn wstring_data(this: &CxxWString) -> *const wchar_t;
    #[link_name = "cxxbridge1$cxx_wstring$length"]
    fn wstring_length(this: &CxxWString) -> usize;
    #[link_name = "cxxbridge1$cxx_wstring$clear"]
    fn wstring_clear(this: Pin<&mut CxxWString>);
    #[link_name = "cxxbridge1$cxx_wstring$reserve_total"]
    fn wstring_reserve_total(this: Pin<&mut CxxWString>, new_cap: usize);
    #[link_name = "cxxbridge1$cxx_wstring$push"]
    fn wstring_push(this: Pin<&mut CxxWString>, ptr: *const wchar_t, len: usize);
    #[link_name = "cxxbridge1$cxx_wstring$new"]
    fn wstring_new(ptr: *const wchar_t, len: usize) -> *mut ::cxx::core::ffi::c_void;

}

/// Binding to C++ `std::string`.
///
/// # Invariants
///
/// As an invariant of this API and the static analysis of the cxx::bridge
/// macro, in Rust code we can never obtain a `CxxWString` by value. C++'s string
/// requires a move constructor and may hold internal pointers, which is not
/// compatible with Rust's move behavior. Instead in Rust code we will only ever
/// look at a CxxWString through a reference or smart pointer, as in `&CxxWString`
/// or `UniquePtr<CxxWString>`.
#[repr(C)]
pub struct CxxWString {
    _private: [u8; 0],
    _pinned: PhantomData<PhantomPinned>,
}

/// Construct a C++ std::wstring on the Rust stack.
///
/// # Syntax
///
/// In statement position:
///
/// ```
/// # use cxx::let_cxx_wstring;
/// # let expression = "";
/// let_cxx_string!(var = expression);
/// ```
///
/// The `expression` may have any type that implements `AsRef<[char]>`.
///
/// The macro expands to something resembling `let $var: Pin<&mut CxxWString> =
/// /*???*/;`. The resulting [`Pin`] can be deref'd to `&CxxWString` as needed.
///
/// # Example
///
/// ```
/// use cxx::{let_cxx_wstring, CxxWString};
///
/// fn f(s: &CxxWString) {/* ... */}
///
/// fn main() {
///     let_cxx_wstring!(s = "example");
///     f(&s);
/// }
/// ```
#[macro_export]
macro_rules! let_cxx_wstring {
    ($var:ident = $value:expr $(,)?) => {
        let mut cxx_stack_string = $crate::private::StackWString::new();
        #[allow(unused_mut, unused_unsafe)]
        let mut $var = match $value {
            let_cxx_string => unsafe { cxx_stack_string.init(let_cxx_string) },
        };
    };
}

impl CxxWString {
    /// `CxxWString` is not constructible via `new`. Instead, use the
    /// [`let_cxx_wstring!`] macro.
    pub fn new<T: Private>() -> Self {
        unreachable!()
    }

    /// Returns the length of the string in bytes.
    ///
    /// Matches the behavior of C++ [std::string::size][size].
    ///
    /// [size]: https://en.cppreference.com/w/cpp/string/basic_string/size
    pub fn len(&self) -> usize {
        unsafe { wstring_length(self) }
    }

    /// Returns true if `self` has a length of zero bytes.
    ///
    /// Matches the behavior of C++ [std::string::empty][empty].
    ///
    /// [empty]: https://en.cppreference.com/w/cpp/string/basic_string/empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a wchar slice of this string's contents.
    pub fn as_wchars(&self) -> &[wchar_t] {
        let data = self.as_ptr();
        let len = self.len();
        unsafe { slice::from_raw_parts(data, len) }
    }

    /// Returns a char slice of this string's contents.
    pub fn as_chars(&self) -> &[char] {
        let data = self.as_ptr();
        let len = self.len();
        unsafe { slice::from_raw_parts(data as *const char, len) }
    }

    /// Helper to construct a char iterator, simplifying some other methods.
    fn as_char_iter(&self) -> impl Iterator<Item = char> + '_ {
        self.as_chars().iter().copied()
    }

    /// Produces a pointer to the first character of the string.
    ///
    /// Matches the behavior of C++ [std::string::data][data].
    ///
    /// Note that the return type may look like `const char *` but is not a
    /// `const char *` in the typical C sense, as C++ strings may contain
    /// internal null bytes. As such, the returned pointer only makes sense as a
    /// string in combination with the length returned by [`len()`][len].
    ///
    /// [data]: https://en.cppreference.com/w/cpp/string/basic_string/data
    /// [len]: #method.len
    pub fn as_ptr(&self) -> *const wchar_t {
        unsafe { wstring_data(self) }
    }

    /// Validates that the C++ string contains UTF-8 data and produces a view of
    /// it as a Rust &amp;str, otherwise an error.
    // pub fn to_str(&self) -> Result<&str, Utf8Error> {
    //     str::from_utf8(self.as_bytes())
    // }
    pub fn to_str(&self) -> String {
        return self.as_chars().iter().collect();
    }

    /// If the contents of the C++ string are valid UTF-8, this function returns
    /// a view as a Cow::Borrowed &amp;str. Otherwise replaces any invalid UTF-8
    /// sequences with the U+FFFD [replacement character] and returns a
    /// Cow::Owned String.
    ///
    /// [replacement character]: https://doc.rust-lang.org/std/char/constant.REPLACEMENT_CHARACTER.html
    // #[cfg(feature = "alloc")]
    // #[cfg_attr(doc_cfg, doc(cfg(feature = "alloc")))]
    // pub fn to_string_lossy(&self) -> Cow<str> {
    //     String::from_utf8_lossy(self.as_bytes())
    // }

    /// Removes all characters from the string.
    ///
    /// Matches the behavior of C++ [std::string::clear][clear].
    ///
    /// Note: **unlike** the guarantee of Rust's `std::string::String::clear`,
    /// the C++ standard does not require that capacity is unchanged by this
    /// operation. In practice existing implementations do not change the
    /// capacity but all pointers, references, and iterators into the string
    /// contents are nevertheless invalidated.
    ///
    /// [clear]: https://en.cppreference.com/w/cpp/string/basic_string/clear
    pub fn clear(self: Pin<&mut Self>) {
        unsafe { wstring_clear(self) }
    }

    /// Ensures that this string's capacity is at least `additional` bytes
    /// larger than its length.
    ///
    /// The capacity may be increased by more than `additional` bytes if it
    /// chooses, to amortize the cost of frequent reallocations.
    ///
    /// **The meaning of the argument is not the same as
    /// [std::string::reserve][reserve] in C++.** The C++ standard library and
    /// Rust standard library both have a `reserve` method on strings, but in
    /// C++ code the argument always refers to total capacity, whereas in Rust
    /// code it always refers to additional capacity. This API on `CxxWString`
    /// follows the Rust convention, the same way that for the length accessor
    /// we use the Rust conventional `len()` naming and not C++ `size()` or
    /// `length()`.
    ///
    /// # Panics
    ///
    /// Panics if the new capacity overflows usize.
    ///
    /// [reserve]: https://en.cppreference.com/w/cpp/string/basic_string/reserve
    pub fn reserve(self: Pin<&mut Self>, additional: usize) {
        let new_cap = self
            .len()
            .checked_add(additional)
            .expect("CxxWString capacity overflow");
        unsafe { wstring_reserve_total(self, new_cap) }
    }

    /// Appends a given string slice onto the end of this C++ string.
    pub fn push_str(self: Pin<&mut Self>, s: &str) {
        let chars = s.chars().collect::<std::vec::Vec<_>>();
        self.push_chars(&chars);
    }

    /// Appends arbitrary chars onto the end of this C++ string.
    pub fn push_chars(self: Pin<&mut Self>, chars: &[char]) {
        unsafe { wstring_push(self, chars.as_ptr() as *const wchar_t, chars.len()) }
    }

    /// Create a UniquePtr<CxxWString> from a slice of chars.
    pub fn create(chars: &[char]) -> UniquePtr<Self> {
        unsafe {
            UniquePtr::from_raw(
                wstring_new(chars.as_ptr() as *const wchar_t, chars.len()) as *mut CxxWString
            )
        }
    }
}

impl Display for CxxWString {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.to_str())
    }
}

impl Debug for CxxWString {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.to_str())
    }
}

impl PartialEq for CxxWString {
    fn eq(&self, other: &Self) -> bool {
        self.as_wchars() == other.as_wchars()
    }
}

impl PartialEq<CxxWString> for str {
    fn eq(&self, other: &CxxWString) -> bool {
        self.chars().eq(other.as_char_iter())
    }
}

impl PartialEq<str> for CxxWString {
    fn eq(&self, other: &str) -> bool {
        other.chars().eq(self.as_char_iter())
    }
}

macro_rules! impl_partial_eq {
    ($($ty:ty),*) => {
        $(
            impl PartialEq<$ty> for CxxWString {
                fn eq(&self, other: &$ty) -> bool {
                    self.as_wchars() == other.as_slice()
                }
            }

            impl PartialEq<CxxWString> for $ty {
                fn eq(&self, other: &CxxWString) -> bool {
                    self.as_slice() == other.as_wchars()
                }
            }
        )*
    }
}

impl_partial_eq!(U32CStr, U32CString, Utf32Str, Utf32String);

impl Eq for CxxWString {}

impl PartialOrd for CxxWString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.as_chars().partial_cmp(other.as_chars())
    }
}

impl Ord for CxxWString {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_chars().cmp(other.as_chars())
    }
}

impl Hash for CxxWString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_chars().hash(state);
    }
}

#[doc(hidden)]
#[repr(C)]
pub struct StackWString {
    // Static assertions in cxx.cc validate that this is large enough and
    // aligned enough.
    space: MaybeUninit<[usize; 8]>,
}

#[allow(missing_docs)]
impl StackWString {
    pub fn new() -> Self {
        StackWString {
            space: MaybeUninit::uninit(),
        }
    }

    pub unsafe fn init(&mut self, value: impl AsRef<[char]>) -> Pin<&mut CxxWString> {
        let value = value.as_ref();
        unsafe {
            let this = &mut *self.space.as_mut_ptr().cast::<MaybeUninit<CxxWString>>();
            wstring_init(this, value.as_ptr() as *const wchar_t, value.len());
            Pin::new_unchecked(&mut *this.as_mut_ptr())
        }
    }
}

impl Drop for StackWString {
    fn drop(&mut self) {
        unsafe {
            let this = &mut *self.space.as_mut_ptr().cast::<MaybeUninit<CxxWString>>();
            wstring_destroy(this);
        }
    }
}
