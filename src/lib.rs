#![feature(extern_types, ptr_metadata, unsize, coerce_unsized, test)]

use std::fmt;
use std::fmt::Debug;
use std::marker::Unsize;
use std::mem;
use std::ops::{CoerceUnsized, Deref, DerefMut};
use std::ptr::{self, Pointee};
use std::error::Error;

#[repr(transparent)]
pub struct NarrowBox<Dyn: ?Sized>(ptr::NonNull<Erased<Dyn>>);

// Repr represents a Dyn. used here as either
// - Repr<Dyn, T> where T unsizes to Dyn
// - Repr<Dyn, Dyn>, unsized, for grabbing a wide pointer to the Dyn
// - Repr<T, T>, sized, when nothing dynamic is going on
// - Repr<Dyn, ErasedExtern>, for narrow pointers to Reprs of unsized things
//
// Unless Dyn is Sized, metadata has to hold the metadata to go from the
// erased version to the unsized version. If it is Sized, don't look at
// metadata, we didn't set it.
#[repr(C)]
struct Repr<Dyn: ?Sized, T: ?Sized> {
	metadata: ReprMeta<Dyn>,
	inner: T,
}

extern "C" {
	type ErasedExtern;
}

// Repr that's possibly unsized. I guess I mean ?Sized here but I can't use
// that in an identifier and questionmarksized is too long.
type Unsized<Dyn> = Repr<Dyn, Dyn>;
// Repr that's sized, but has metadata to go to the above.
type Erased<Dyn> = Repr<Dyn, ErasedExtern>;

// Either just the metadata for unsized Repr, or nothing if Dyn is actually
// sized, but leaving enough room that we can unsize in-place by writing in
// the right metadata.
union ReprMeta<Dyn: ?Sized> {
	actual: <Unsized<Dyn> as Pointee>::Metadata,
	dummy: usize,
}

// Unsize by forgetting T
impl<Dyn: ?Sized, T> CoerceUnsized<Unsized<Dyn>>
	for Repr<Dyn, T>
	where T: CoerceUnsized<Dyn> {}

// Get metadata for T unsized into Dyn out of thin air.
//
// This is probably legal?
fn synthesize_metadata<Dyn: ?Sized, T: Unsize<Dyn>>()
		-> <Dyn as Pointee>::Metadata {
	let narrow_dummy: *const T = ptr::null();
	let wide_dummy: *const Dyn = narrow_dummy;
	ptr::metadata(wide_dummy)
}

// NarrowBox for something Sized. not sure if you'd actually want this but you
// can unsize it into the unsized version without reallocating, so maybe it's
// useful if you're starting with a concrete type that you later want to
// forget.
impl<T: Sized> NarrowBox<T> {
	pub fn new(inner: T) -> NarrowBox<T> {
		unsafe { Self::new_with_meta(inner, ReprMeta { dummy: 0 }) }
	}

	// unsize T into Dyn, stash the metadata, then forget about T
	pub fn unsize<Dyn: ?Sized>(self) -> NarrowBox<Dyn> where T: Unsize<Dyn> {
		unsafe {
			let erased = self.into_raw_erased() as *mut Erased<Dyn>;
			(*erased).metadata.actual =
				synthesize_metadata::<Unsized<Dyn>, Repr<Dyn, T>>();
			let boxed = NarrowBox(ptr::NonNull::new_unchecked(erased));
			boxed
		}
	}

	pub fn into_inner(self) -> T {
		let boxed = unsafe { Box::from_raw(self.into_raw_unsized()) };
		boxed.inner
	}
}

impl<Dyn: ?Sized> NarrowBox<Dyn> {
	// box a T, unsize to Box<Dyn>, stash the metadata for that and return
	// a narrow pointer to the box
	pub fn new_unsize<T>(inner: T) -> NarrowBox<Dyn> where T: Unsize<Dyn> {
		let metadata = ReprMeta {
			actual: synthesize_metadata::<Unsized<Dyn>, Repr<Dyn, T>>(),
		};
		unsafe { Self::new_with_meta(inner, metadata) }
	}

	// must be the right metadata; just save me 3 lines in the above
	unsafe fn new_with_meta<T>(inner: T, metadata: ReprMeta<Dyn>)
			-> NarrowBox<Dyn> {
		let boxed = Box::new(Repr { metadata, inner });
		let erased = Box::into_raw(boxed) as *mut Erased<Dyn>;

		NarrowBox(ptr::NonNull::new(erased).unwrap())
	}

	// forget the stashed metadata, just pretend this was a T all along.
	//
	// maybe use into_inner next to unbox.
	pub unsafe fn downcast_unchecked<T>(self) -> NarrowBox<T> {
		let p = self.into_raw_erased() as *mut Erased<T>;
		let boxed = NarrowBox(ptr::NonNull::new_unchecked(p));
		boxed
	}

	pub unsafe fn downcast_ref_unchecked<T>(&self) -> &T {
		let p = self.0.as_ptr() as *mut Repr<Dyn, T>;
		&(*p).inner
	}

	pub unsafe fn downcast_mut_unchecked<T>(&mut self) -> &mut T {
		let p = self.0.as_ptr() as *mut Repr<Dyn, T>;
		&mut (*p).inner
	}

	// use the stashed metadata to get a wide raw pointer to the Repr
	fn get_raw_unsized(&self) -> *mut Unsized<Dyn> {
		unsafe {
			let p = self.0.as_ptr();
			ptr::from_raw_parts_mut(p as *mut (), (*p).metadata.actual)
		}
	}

	// as above but forget self
	fn into_raw_unsized(self) -> *mut Unsized<Dyn> {
		let self_ = mem::ManuallyDrop::new(self);
		self_.get_raw_unsized()
	}

	// just get the Repr really just for consistency with the above/below
	fn get_raw_erased(&self) -> *mut Erased<Dyn> {
		self.0.as_ptr()
	}

	// as above but forget self
	fn into_raw_erased(self) -> *mut Erased<Dyn> {
		let self_ = mem::ManuallyDrop::new(self);
		self_.get_raw_erased()
	}
}

impl NarrowBox<dyn Error+'static> {
	fn downcast<T: Error+'static>(self) -> Result<NarrowBox<T>, Self> {
		match self.is::<T>() {
			true => Ok(unsafe { self.downcast_unchecked() }),
			false => Err(self),
		}
	}
}

// drop by restoring the original wide Box, which knows how to drop/deallocate
impl<Dyn: ?Sized> Drop for NarrowBox<Dyn> {
	fn drop(&mut self) {
		unsafe { Box::from_raw(self.get_raw_unsized()); }
	}
}

unsafe impl<Dyn: Send+?Sized> Send for NarrowBox<Dyn> {}
unsafe impl<Dyn: Sync+?Sized> Sync for NarrowBox<Dyn> {}

// idk pretend i also implemented Borrow and AsRef and so on?

impl<Dyn: ?Sized> Deref for NarrowBox<Dyn> {
	type Target = Dyn;
	fn deref(&self) -> &Dyn {
		unsafe { &(*self.get_raw_unsized()).inner }
	}
}

impl<Dyn: ?Sized> DerefMut for NarrowBox<Dyn> {
	fn deref_mut(&mut self) -> &mut Dyn {
		unsafe { &mut (*self.get_raw_unsized()).inner }
	}
}

// probably want to forward a million traits here ugh

impl<Dyn: ?Sized + Debug> Debug for NarrowBox<Dyn> {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		self.deref().fmt(f)
	}
}

#[cfg(test)]
mod test {
	extern crate test;
	use super::*;
	use test::Bencher;
	use std::io;

	#[derive(Debug)]
	#[repr(align(1024))]
	struct Loud(String);

	impl Drop for Loud {
		fn drop(&mut self) {
			eprintln!("dropping {:?}", self);
			assert!(self.0 != "oh no");
			self.0 = "oh no".to_string();
		}
	}

	fn compare_meta<Dyn: ?Sized, T: Unsize<Dyn>>() {
		unsafe {
			let m1: usize = mem::transmute_copy(&synthesize_metadata::<Dyn, T>());
			let m2: usize = mem::transmute_copy(&synthesize_metadata::<Repr<Dyn, Dyn>, Repr<Dyn, T>>());
			assert_eq!(m1, m2);
		}
	}

	#[test]
	fn main() {
		// just out of curiosity, looks like the metadata is identical for a thing
		// and our Repr warpping that thing, makes sense I guess
		compare_meta::<[i32], [i32; 5]>();
		compare_meta::<dyn Debug, Loud>();

		// unsize array
		let ary = [1, 2, 3, 4, 5, 6];
		let boxed: NarrowBox<[i32]> = NarrowBox::new_unsize(ary);
		// yet its narrow
		assert_eq!(mem::size_of_val(&boxed), mem::size_of::<*const ()>());
		// we can still look at it
		dbg!(boxed);

		// unsized into trait object
		let err = std::fs::read("/lmao").err().unwrap();
		let boxed: NarrowBox<dyn Error+'static> = NarrowBox::new_unsize(err);
		// still narrow
		assert_eq!(mem::size_of_val(&boxed), mem::size_of::<*const ()>());
		// still debuggable
		dbg!(&boxed);
		// we can use Error's downcast_ref()
		dbg!(boxed.downcast_ref::<io::Error>().expect("hmm"));
		// we can use our downcast using Error's is()
		dbg!(boxed.downcast::<io::Error>().expect("hmm"));

		// starting with a sized narrowbox and then unsizing also works
		let err = std::fs::read("/lmao").err().unwrap();
		let boxed: NarrowBox<io::Error> = NarrowBox::new(err);
		let boxed: NarrowBox<dyn Error+'static> = boxed.unsize();
		dbg!(boxed.downcast_ref::<io::Error>().expect("hmm"));
		dbg!(boxed.downcast::<io::Error>().expect("hmm"));

		// we're actually dropping things
		NarrowBox::<dyn Debug>::new_unsize(Loud("neat".to_string()));

		// ok it's a bit unfortunate that is this 2x alignof Loud
		dbg!(mem::size_of::<Repr<dyn Debug, Loud>>());
		dbg!(mem::size_of::<Repr<Loud, Loud>>());

		NarrowBox::new(Loud("ok!".to_string())).into_inner();
	}

	#[bench]
	fn normal_box(b: &mut Bencher) {
		b.iter(|| {
			for _ in 0..(1<<16) {
				let mut outer = Box::new([(); 10].map(|_| None));
				for item in outer.iter_mut() {
					*item = Some(Box::new(String::new()));
				}
				test::black_box(outer);

				let mut outer: Box<[Option<Box<dyn Debug>>]> = Box::new([(); 10].map(|_| None));
				for item in outer.iter_mut() {
					*item = Some(Box::new("lol"));
				}
				test::black_box(outer);
			}
		});
	}
	#[bench]
	fn narrow_box(b: &mut Bencher) {
		b.iter(|| {
			for _ in 0..(1<<16) {
				let mut outer = NarrowBox::new([(); 10].map(|_| None));
				for item in outer.iter_mut() {
					*item = Some(NarrowBox::new(String::new()));
				}
				test::black_box(outer);

				let mut outer: NarrowBox<[Option<NarrowBox<dyn Debug>>]> = NarrowBox::new_unsize([(); 10].map(|_| None));
				for item in outer.iter_mut() {
					*item = Some(NarrowBox::new_unsize("lol"));
				}
				test::black_box(outer);
			}
		});
	}
}
