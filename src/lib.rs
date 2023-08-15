//! This crate implements a structure that can be used as a generic array type.
//!
//! Before Rust 1.51, arrays `[T; N]` were problematic in that they couldn't be generic with respect to the length `N`, so this wouldn't work:
//!
//! ```rust{compile_fail}
//! struct Foo<N> {
//!     data: [i32; N],
//! }
//! ```
//!
//! Since 1.51, the below syntax is valid:
//!
//! ```rust
//! struct Foo<const N: usize> {
//!     data: [i32; N],
//! }
//! ```
//!
//! However, the const-generics we have as of writing this are still the minimum-viable product (`min_const_generics`), so many situations still result in errors, such as this example:
//!
//! ```compile_fail
//! # struct Foo<const N: usize> {
//! #   data: [i32; N],
//! # }
//! trait Bar {
//!     const LEN: usize;
//!
//!     // Error: cannot perform const operation using `Self`
//!     fn bar(&self) -> Foo<{ Self::LEN }>;
//! }
//! ```
//!
//! **generic-array** defines a new trait [`ArrayLength`] and a struct [`GenericArray<T, N: ArrayLength>`](GenericArray),
//! which lets the above be implemented as:
//!
//! ```rust
//! use generic_array::{GenericArray, ArrayLength};
//!
//! struct Foo<N: ArrayLength> {
//!     data: GenericArray<i32, N>
//! }
//!
//! trait Bar {
//!     type LEN: ArrayLength;
//!     fn bar(&self) -> Foo<Self::LEN>;
//! }
//! ```
//!
//! The [`ArrayLength`] trait is implemented for
//! [unsigned integer types](typenum::Unsigned) from
//! [typenum](typenum):
//!
//! ```rust
//! # use generic_array::{ArrayLength, GenericArray};
//! use generic_array::typenum::U5;
//!
//! struct Foo<N: ArrayLength> {
//!     data: GenericArray<i32, N>
//! }
//!
//! let foo = Foo::<U5>{data: GenericArray::default()};
//! ```
//!
//! For example, [`GenericArray<T, U5>`] would work almost like `[T; 5]`:
//!
//! ```rust
//! # use generic_array::{ArrayLength, GenericArray};
//! use generic_array::typenum::U5;
//!
//! struct Foo<T, N: ArrayLength> {
//!     data: GenericArray<T, N>
//! }
//!
//! let foo = Foo::<i32, U5>{data: GenericArray::default()};
//! ```
//!
//! The `arr!` macro is provided to allow easier creation of literal arrays, as shown below:
//!
//! ```rust
//! # use generic_array::arr;
//! let array = arr![1, 2, 3];
//! //  array: GenericArray<i32, typenum::U3>
//! assert_eq!(array[2], 3);
//! ```
//! ## Feature flags
//!
//! ```toml
//! [dependencies.generic-array]
//! features = [
//!     "more_lengths",  # Expands From/Into implementation for more array lengths
//!     "serde",         # Serialize/Deserialize implementation
//!     "zeroize",       # Zeroize implementation for setting array elements to zero
//!     "const-default", # Compile-time const default value support via trait
//!     "alloc"          # Enables From/TryFrom implementations between GenericArray and Vec<T>/Box<[T]>
//! ]
//! ```

#![deny(missing_docs)]
#![deny(meta_variable_misuse)]
#![no_std]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

pub extern crate typenum;

#[doc(hidden)]
#[cfg(feature = "alloc")]
pub extern crate alloc;

mod hex;
mod impls;
mod iter;

#[cfg(feature = "alloc")]
mod impl_alloc;

#[cfg(feature = "const-default")]
mod impl_const_default;

#[cfg(feature = "serde")]
mod impl_serde;

#[cfg(feature = "zeroize")]
mod impl_zeroize;

use core::iter::FromIterator;
use core::marker::PhantomData;
use core::mem::{ManuallyDrop, MaybeUninit};
use core::ops::{Deref, DerefMut};
use core::{mem, ptr, slice};
use typenum::bit::{B0, B1};
use typenum::generic_const_mappings::{Const, ToUInt, U};
use typenum::uint::{UInt, UTerm, Unsigned};

#[doc(hidden)]
#[cfg_attr(test, macro_use)]
pub mod arr;

pub mod functional;
pub mod sequence;

mod internal;
use internal::{ArrayBuilder, ArrayConsumer, Sealed};

// re-export to allow doc_auto_cfg to handle it
#[cfg(feature = "internals")]
pub mod internals {
    //! Very unsafe internal functionality.
    //!
    //! These are used internally for building and consuming generic arrays. WHen used correctly,
    //! they can ensure elements are correctly dropped if something panics while using them.

    pub use crate::internal::{ArrayBuilder, ArrayConsumer};
}

use self::functional::*;
use self::sequence::*;

pub use self::iter::GenericArrayIter;

/// Trait used to define the number of elements in a [`GenericArray`].
///
/// `ArrayLength` is a superset of [`typenum::Unsigned`].
///
/// Consider `N: ArrayLength` to be equivalent to `const N: usize`
///
/// ```
/// # use generic_array::{GenericArray, ArrayLength};
/// fn foo<N: ArrayLength>(arr: GenericArray<i32, N>) -> i32 {
///     arr.iter().sum()
/// }
/// ```
/// is equivalent to:
/// ```
/// fn foo<const N: usize>(arr: [i32; N]) -> i32 {
///     arr.iter().sum()
/// }
/// ```
///
/// # Safety
///
/// This trait is effectively sealed due to only being allowed on [`Unsigned`] types,
/// and therefore cannot be implemented in user code.
pub unsafe trait ArrayLength: Unsigned + 'static {
    /// Associated type representing the array type with the given number of elements.
    ///
    /// This is an implementation detail, but is required to be public in cases where certain attributes
    /// of the inner type of [`GenericArray`] cannot be proven, such as [`Copy`] bounds.
    ///
    /// [`Copy`] example:
    /// ```
    /// # use generic_array::{GenericArray, ArrayLength};
    /// struct MyType<N: ArrayLength> {
    ///     data: GenericArray<f32, N>,
    /// }
    ///
    /// impl<N: ArrayLength> Clone for MyType<N> where N::ArrayType<f32>: Copy {
    ///     fn clone(&self) -> Self { MyType { ..*self } }
    /// }
    ///
    /// impl<N: ArrayLength> Copy for MyType<N> where N::ArrayType<f32>: Copy {}
    /// ```
    ///
    /// Alternatively, using the entire `GenericArray<f32, N>` type as the bounds works:
    /// ```ignore
    /// where GenericArray<f32, N>: Copy
    /// ```
    type ArrayType<T>: Sealed;
}

unsafe impl ArrayLength for UTerm {
    #[doc(hidden)]
    type ArrayType<T> = [T; 0];
}

/// Implemented for types which can have an associated [`ArrayLength`],
/// such as [`Const<N>`] for use with const-generics.
///
/// ```
/// use generic_array::{GenericArray, IntoArrayLength, ConstArrayLength, typenum::Const};
///
/// fn some_array_interopt<const N: usize>(value: [u32; N]) -> GenericArray<u32, ConstArrayLength<N>>
/// where
///     Const<N>: IntoArrayLength,
/// {
///     let ga = GenericArray::from(value);
///     // do stuff
///     ga
/// }
/// ```
///
/// This is mostly to simplify the `where` bounds, equivalent to:
///
/// ```
/// use generic_array::{GenericArray, ArrayLength, typenum::{Const, U, ToUInt}};
///
/// fn some_array_interopt<const N: usize>(value: [u32; N]) -> GenericArray<u32, U<N>>
/// where
///     Const<N>: ToUInt,
///     U<N>: ArrayLength,
/// {
///     let ga = GenericArray::from(value);
///     // do stuff
///     ga
/// }
/// ```
pub trait IntoArrayLength {
    /// The associated `ArrayLength`
    type ArrayLength: ArrayLength;
}

impl<const N: usize> IntoArrayLength for Const<N>
where
    Const<N>: ToUInt,
    U<N>: ArrayLength,
{
    type ArrayLength = U<N>;
}

impl<T> IntoArrayLength for T
where
    T: ArrayLength,
{
    type ArrayLength = Self;
}

/// Associated [`ArrayLength`] for one [`Const<N>`]
///
/// See [`IntoArrayLength`] for more information.
pub type ConstArrayLength<const N: usize> = <Const<N> as IntoArrayLength>::ArrayLength;

/// Internal type used to generate a struct of appropriate size
#[allow(dead_code)]
#[repr(C)]
#[doc(hidden)]
pub struct GenericArrayImplEven<T, U> {
    parent1: U,
    parent2: U,
    _marker: PhantomData<T>,
}

/// Internal type used to generate a struct of appropriate size
#[allow(dead_code)]
#[repr(C)]
#[doc(hidden)]
pub struct GenericArrayImplOdd<T, U> {
    parent1: U,
    parent2: U,
    data: T,
}

impl<T: Clone, U: Clone> Clone for GenericArrayImplEven<T, U> {
    #[inline(always)]
    fn clone(&self) -> GenericArrayImplEven<T, U> {
        // Clone is never called on the GenericArrayImpl types,
        // as we use `self.map(clone)` elsewhere. This helps avoid
        // extra codegen for recursive clones when they are never used.
        unsafe { core::hint::unreachable_unchecked() }
    }
}

impl<T: Clone, U: Clone> Clone for GenericArrayImplOdd<T, U> {
    #[inline(always)]
    fn clone(&self) -> GenericArrayImplOdd<T, U> {
        unsafe { core::hint::unreachable_unchecked() }
    }
}

// Even if Clone is never used, they can still be byte-copyable.
impl<T: Copy, U: Copy> Copy for GenericArrayImplEven<T, U> {}
impl<T: Copy, U: Copy> Copy for GenericArrayImplOdd<T, U> {}

impl<T, U> Sealed for GenericArrayImplEven<T, U> {}
impl<T, U> Sealed for GenericArrayImplOdd<T, U> {}

unsafe impl<N: ArrayLength> ArrayLength for UInt<N, B0> {
    #[doc(hidden)]
    type ArrayType<T> = GenericArrayImplEven<T, N::ArrayType<T>>;
}

unsafe impl<N: ArrayLength> ArrayLength for UInt<N, B1> {
    #[doc(hidden)]
    type ArrayType<T> = GenericArrayImplOdd<T, N::ArrayType<T>>;
}

/// Struct representing a generic array - `GenericArray<T, N>` works like `[T; N]`
///
/// For how to implement [`Copy`] on structs using generic-lengthed `GenericArray` internally, see
/// the docs for [`ArrayLength::ArrayType`].
#[repr(transparent)]
pub struct GenericArray<T, U: ArrayLength> {
    #[allow(dead_code)] // data is never accessed directly
    data: U::ArrayType<T>,
}

unsafe impl<T: Send, N: ArrayLength> Send for GenericArray<T, N> {}
unsafe impl<T: Sync, N: ArrayLength> Sync for GenericArray<T, N> {}

impl<T, N: ArrayLength> Deref for GenericArray<T, N> {
    type Target = [T];

    #[inline(always)]
    fn deref(&self) -> &[T] {
        GenericArray::as_slice(self)
    }
}

impl<T, N: ArrayLength> DerefMut for GenericArray<T, N> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut [T] {
        GenericArray::as_mut_slice(self)
    }
}

impl<'a, T: 'a, N: ArrayLength> IntoIterator for &'a GenericArray<T, N> {
    type IntoIter = slice::Iter<'a, T>;
    type Item = &'a T;

    fn into_iter(self: &'a GenericArray<T, N>) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'a, T: 'a, N: ArrayLength> IntoIterator for &'a mut GenericArray<T, N> {
    type IntoIter = slice::IterMut<'a, T>;
    type Item = &'a mut T;

    fn into_iter(self: &'a mut GenericArray<T, N>) -> Self::IntoIter {
        self.as_mut_slice().iter_mut()
    }
}

impl<T, N: ArrayLength> FromIterator<T> for GenericArray<T, N> {
    fn from_iter<I>(iter: I) -> GenericArray<T, N>
    where
        I: IntoIterator<Item = T>,
    {
        let mut iter = iter.into_iter();

        unsafe {
            let mut destination = ArrayBuilder::new();

            let (destination_iter, position) = destination.iter_position();

            // .zip acts as an automatic .take(N::USIZE)
            destination_iter.zip(&mut iter).for_each(|(dst, src)| {
                dst.write(src);
                *position += 1;
            });

            if *position < N::USIZE {
                from_iter_length_fail(*position, N::USIZE);
            }

            if iter.next().is_some() {
                from_iter_length_fail(N::USIZE + 1, N::USIZE);
            }

            destination.into_inner()
        }
    }
}

#[inline(never)]
#[cold]
pub(crate) fn from_iter_length_fail(length: usize, expected: usize) -> ! {
    panic!(
        "GenericArray::from_iter received {} elements but expected {}",
        length, expected
    );
}

unsafe impl<T, N: ArrayLength> GenericSequence<T> for GenericArray<T, N>
where
    Self: IntoIterator<Item = T>,
{
    type Length = N;
    type Sequence = Self;

    fn generate<F>(mut f: F) -> GenericArray<T, N>
    where
        F: FnMut(usize) -> T,
    {
        unsafe {
            let mut destination = ArrayBuilder::new();

            {
                let (destination_iter, position) = destination.iter_position();

                destination_iter.enumerate().for_each(|(i, dst)| {
                    dst.write(f(i));
                    *position += 1;
                });
            }

            destination.into_inner()
        }
    }

    #[inline(always)]
    fn inverted_zip<B, U, F>(
        self,
        lhs: GenericArray<B, Self::Length>,
        mut f: F,
    ) -> MappedSequence<GenericArray<B, Self::Length>, B, U>
    where
        GenericArray<B, Self::Length>:
            GenericSequence<B, Length = Self::Length> + MappedGenericSequence<B, U>,
        Self: MappedGenericSequence<T, U>,
        Self::Length: ArrayLength,
        F: FnMut(B, Self::Item) -> U,
    {
        unsafe {
            let mut left = ArrayConsumer::new(lhs);
            let mut right = ArrayConsumer::new(self);

            let (left_array_iter, left_position) = left.iter_position();
            let (right_array_iter, right_position) = right.iter_position();

            FromIterator::from_iter(left_array_iter.zip(right_array_iter).map(|(l, r)| {
                let left_value = ptr::read(l);
                let right_value = ptr::read(r);

                *left_position += 1;
                *right_position += 1;

                f(left_value, right_value)
            }))
        }
    }

    #[inline(always)]
    fn inverted_zip2<B, Lhs, U, F>(self, lhs: Lhs, mut f: F) -> MappedSequence<Lhs, B, U>
    where
        Lhs: GenericSequence<B, Length = Self::Length> + MappedGenericSequence<B, U>,
        Self: MappedGenericSequence<T, U>,
        Self::Length: ArrayLength,
        F: FnMut(Lhs::Item, Self::Item) -> U,
    {
        unsafe {
            let mut right = ArrayConsumer::new(self);

            let (right_array_iter, right_position) = right.iter_position();

            FromIterator::from_iter(
                lhs.into_iter()
                    .zip(right_array_iter)
                    .map(|(left_value, r)| {
                        let right_value = ptr::read(r);

                        *right_position += 1;

                        f(left_value, right_value)
                    }),
            )
        }
    }
}

impl<T, U, N: ArrayLength> MappedGenericSequence<T, U> for GenericArray<T, N>
where
    GenericArray<U, N>: GenericSequence<U, Length = N>,
{
    type Mapped = GenericArray<U, N>;
}

impl<T, N: ArrayLength> FunctionalSequence<T> for GenericArray<T, N>
where
    Self: GenericSequence<T, Item = T, Length = N>,
{
    fn map<U, F>(self, mut f: F) -> MappedSequence<Self, T, U>
    where
        Self::Length: ArrayLength,
        Self: MappedGenericSequence<T, U>,
        F: FnMut(T) -> U,
    {
        unsafe {
            let mut source = ArrayConsumer::new(self);

            let (array_iter, position) = source.iter_position();

            FromIterator::from_iter(array_iter.map(|src| {
                let value = ptr::read(src);

                *position += 1;

                f(value)
            }))
        }
    }

    #[inline]
    fn zip<B, Rhs, U, F>(self, rhs: Rhs, f: F) -> MappedSequence<Self, T, U>
    where
        Self: MappedGenericSequence<T, U>,
        Rhs: MappedGenericSequence<B, U, Mapped = MappedSequence<Self, T, U>>,
        Self::Length: ArrayLength,
        Rhs: GenericSequence<B, Length = Self::Length>,
        F: FnMut(T, Rhs::Item) -> U,
    {
        rhs.inverted_zip(self, f)
    }

    fn fold<U, F>(self, init: U, mut f: F) -> U
    where
        F: FnMut(U, T) -> U,
    {
        unsafe {
            let mut source = ArrayConsumer::new(self);

            let (array_iter, position) = source.iter_position();

            array_iter.fold(init, |acc, src| {
                let value = ptr::read(src);
                *position += 1;
                f(acc, value)
            })
        }
    }
}

impl<T, N: ArrayLength> GenericArray<T, N> {
    /// Extracts a slice containing the entire array.
    #[inline(always)]
    pub const fn as_slice(&self) -> &[T] {
        unsafe { slice::from_raw_parts(self as *const Self as *const T, N::USIZE) }
    }

    /// Extracts a mutable slice containing the entire array.
    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { slice::from_raw_parts_mut(self as *mut Self as *mut T, N::USIZE) }
    }

    /// Converts slice to a generic array reference with inferred length.
    ///
    /// # Panics
    ///
    /// Panics if the slice is not equal to the length of the array.
    ///
    /// Consider [`TryFrom`]/[`TryInto`] for a fallible conversion.
    #[inline(always)]
    pub const fn from_slice(slice: &[T]) -> &GenericArray<T, N> {
        if slice.len() != N::USIZE {
            panic!("slice.len() != N in GenericArray::from_slice");
        }

        unsafe { &*(slice.as_ptr() as *const GenericArray<T, N>) }
    }

    /// Converts mutable slice to a mutable generic array reference with inferred length.
    ///
    /// # Panics
    ///
    /// Panics if the slice is not equal to the length of the array.
    ///
    /// Consider [`TryFrom`]/[`TryInto`] for a fallible conversion.
    #[inline(always)]
    pub fn from_mut_slice(slice: &mut [T]) -> &mut GenericArray<T, N> {
        assert_eq!(
            slice.len(),
            N::USIZE,
            "slice.len() != N in GenericArray::from_mut_slice"
        );

        unsafe { &mut *(slice.as_mut_ptr() as *mut GenericArray<T, N>) }
    }

    /// Convert a native array into `GenericArray` of the same length and type.
    ///
    /// This is the `const` equivalent of using the standard [`From`]/[`Into`] traits methods.
    #[inline(always)]
    pub const fn from_array<const N2: usize>(value: [T; N2]) -> Self
    where
        Const<N2>: IntoArrayLength<ArrayLength = N>,
    {
        unsafe { crate::const_transmute(value) }
    }

    /// Convert the `GenericArray` into a native array of the same length and type.
    ///
    /// This is the `const` equivalent of using the standard [`From`]/[`Into`] traits methods.
    #[inline(always)]
    pub const fn into_array<const N2: usize>(self) -> [T; N2]
    where
        Const<N2>: IntoArrayLength<ArrayLength = N>,
    {
        unsafe { crate::const_transmute(self) }
    }
}

/// Error for [`TryFrom`]
#[derive(Debug, Clone, Copy)]
pub struct LengthError;

// TODO: Impl core::error::Error when when https://github.com/rust-lang/rust/issues/103765 is finished
// impl core::fmt::Display for LengthError {
//     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
//         f.write_str("LengthError: Length of given slice does not match GenericArray length")
//     }
// }

impl<'a, T, N: ArrayLength> TryFrom<&'a [T]> for &'a GenericArray<T, N> {
    type Error = LengthError;

    #[inline(always)]
    fn try_from(slice: &'a [T]) -> Result<Self, Self::Error> {
        if slice.len() == N::USIZE {
            Ok(GenericArray::from_slice(slice))
        } else {
            Err(LengthError)
        }
    }
}

impl<'a, T, N: ArrayLength> TryFrom<&'a mut [T]> for &'a mut GenericArray<T, N> {
    type Error = LengthError;

    #[inline(always)]
    fn try_from(slice: &'a mut [T]) -> Result<Self, Self::Error> {
        if slice.len() == N::USIZE {
            Ok(GenericArray::from_mut_slice(slice))
        } else {
            Err(LengthError)
        }
    }
}

impl<T: Clone, N: ArrayLength> GenericArray<T, N> {
    /// Construct a `GenericArray` from a slice by cloning its content.
    ///
    /// Use [`GenericArray::from_exact_iter(slice.iter().cloned())`](GenericArray::from_exact_iter)
    /// for a fallible version.
    ///
    /// # Panics
    ///
    /// Panics if the slice is not equal to the length of the array.
    #[inline]
    pub fn clone_from_slice(slice: &[T]) -> GenericArray<T, N> {
        Self::from_exact_iter(slice.iter().cloned())
            .expect("Slice must be the same length as the array")
    }
}

impl<T, N: ArrayLength> GenericArray<T, N> {
    /// Creates a new `GenericArray` instance from an iterator with a specific size.
    ///
    /// Returns `None` if the size is not equal to the number of elements in the `GenericArray`.
    pub fn from_exact_iter<I>(iter: I) -> Option<Self>
    where
        I: IntoIterator<Item = T>,
    {
        let mut iter = iter.into_iter();

        unsafe {
            let mut destination = ArrayBuilder::new();

            {
                let (destination_iter, position) = destination.iter_position();

                destination_iter.zip(&mut iter).for_each(|(dst, src)| {
                    dst.write(src);
                    *position += 1;
                });

                // The iterator produced fewer than `N` elements.
                if *position != N::USIZE {
                    return None;
                }

                // The iterator produced more than `N` elements.
                if iter.next().is_some() {
                    return None;
                }
            }

            Some(destination.into_inner())
        }
    }
}

/// A const reimplementation of the [`transmute`](core::mem::transmute) function,
/// avoiding problems when the compiler can't prove equal sizes.
///
/// # Safety
/// Treat this the same as [`transmute`](core::mem::transmute), or (preferably) don't use it at all.
#[inline(always)]
#[cfg_attr(not(feature = "internals"), doc(hidden))]
pub const unsafe fn const_transmute<A, B>(a: A) -> B {
    if mem::size_of::<A>() != mem::size_of::<B>() {
        panic!("Size mismatch for generic_array::const_transmute");
    }

    #[repr(C)]
    union Union<A, B> {
        a: ManuallyDrop<A>,
        b: ManuallyDrop<B>,
    }

    let a = ManuallyDrop::new(a);
    ManuallyDrop::into_inner(Union { a }.b)
}

#[cfg(test)]
mod test {
    // Compile with:
    // cargo rustc --lib --profile test --release --
    //      -C target-cpu=native -C opt-level=3 --emit asm
    // and view the assembly to make sure test_assembly generates
    // SIMD instructions instead of a naive loop.

    #[inline(never)]
    pub fn black_box<T>(val: T) -> T {
        use core::{mem, ptr};

        let ret = unsafe { ptr::read_volatile(&val) };
        mem::forget(val);
        ret
    }

    #[test]
    fn test_assembly() {
        use crate::functional::*;

        let a = black_box(arr![1, 3, 5, 7]);
        let b = black_box(arr![2, 4, 6, 8]);

        let c = (&a).zip(b, |l, r| l + r);

        let d = a.fold(0, |a, x| a + x);

        assert_eq!(c, arr![3, 7, 11, 15]);

        assert_eq!(d, 16);
    }
}
