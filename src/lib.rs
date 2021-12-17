#![feature(extern_types, ptr_metadata, unsize, coerce_unsized, test)]

use std::fmt;
use std::fmt::Debug;
use std::marker::Unsize;
use std::mem;
use std::ops::{CoerceUnsized, Deref, DerefMut};
use std::ptr::{self, Pointee};

#[repr(transparent)]
pub struct NarrowBox<Dyn: ?Sized>(ptr::NonNull<Erased<Dyn>>);

#[repr(C)]
struct Repr<Dyn: ?Sized, T: ?Sized> {
	metadata: ReprMeta<Dyn>,
	inner: T,
}

extern "C" {
	type ErasedExtern;
}

type Unsized<Dyn> = Repr<Dyn, Dyn>;
type Erased<Dyn> = Repr<Dyn, ErasedExtern>;

union ReprMeta<Dyn: ?Sized> {
	actual: <Unsized<Dyn> as Pointee>::Metadata,
	_dummy: usize,
}

impl<Dyn: ?Sized, T> CoerceUnsized<Unsized<Dyn>>
	for Repr<Dyn, T>
	where T: CoerceUnsized<Dyn> {}

fn synthesize_metadata<Dyn: ?Sized, T: Unsize<Dyn>>()
		-> <Dyn as Pointee>::Metadata {
	let narrow_dummy: *const T = ptr::null();
	let wide_dummy: *const Dyn = narrow_dummy;
	ptr::metadata(wide_dummy)
}

impl<T: Sized> NarrowBox<T> {
	pub fn new(inner: T) -> NarrowBox<T> {
		let metadata = ptr::metadata::<Unsized<T>>(ptr::null());
		unsafe { Self::new_with_meta(inner, metadata) }
	}

	pub fn unsize<Dyn: ?Sized>(self) -> NarrowBox<Dyn> where T: Unsize<Dyn> {
		unsafe {
			let erased = self.into_repr() as *mut Erased<Dyn>;
			(*erased).metadata.actual =
				synthesize_metadata::<Unsized<Dyn>, Repr<Dyn, T>>();
			let boxed = NarrowBox(ptr::NonNull::new_unchecked(erased));
			boxed
		}
	}

	pub fn into_inner(self) -> T {
		let boxed = unsafe { Box::from_raw(self.into_repr()) };
		boxed.inner
	}
}

impl<Dyn: ?Sized> NarrowBox<Dyn> {
	pub fn new_unsize<T>(inner: T) -> NarrowBox<Dyn> where T: Unsize<Dyn> {
		let metadata = synthesize_metadata::<Unsized<Dyn>, Repr<Dyn, T>>();
		unsafe { Self::new_with_meta(inner, metadata) }
	}

	pub unsafe fn downcast_unchecked<T>(self) -> NarrowBox<T> {
		let p = self.into_repr() as *mut Erased<T>;
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

	// must be the right metadata
	unsafe fn new_with_meta<T>(inner: T,
				metadata: <Unsized<Dyn> as Pointee>::Metadata)
			-> NarrowBox<Dyn> {
		let metadata = ReprMeta { actual: metadata };
		let boxed = Box::new(Repr { metadata, inner });
		let erased = Box::into_raw(boxed) as *mut Erased<Dyn>;

		NarrowBox(ptr::NonNull::new(erased).unwrap())
	}

	fn repr(&self) -> *mut Repr<Dyn, Dyn> {
		unsafe {
			let p = self.0.as_ptr();
			ptr::from_raw_parts_mut(p as *mut (), (*p).metadata.actual)
		}
	}

	fn into_repr(self) -> *mut Repr<Dyn, Dyn> {
		let self_ = mem::ManuallyDrop::new(self);
		self_.repr()
	}
}

unsafe impl<Dyn: Send+?Sized> Send for NarrowBox<Dyn> {}
unsafe impl<Dyn: Sync+?Sized> Sync for NarrowBox<Dyn> {}

impl<Dyn: ?Sized> Drop for NarrowBox<Dyn> {
	fn drop(&mut self) {
		unsafe { Box::from_raw(self.repr()); }
	}
}

impl<Dyn: ?Sized> Deref for NarrowBox<Dyn> {
	type Target = Dyn;
	fn deref(&self) -> &Dyn {
		// safety: yes
		unsafe { &(*self.repr()).inner }
	}
}

impl<Dyn: ?Sized> DerefMut for NarrowBox<Dyn> {
	fn deref_mut(&mut self) -> &mut Dyn {
		// safety: inner doesnt return null, also we mut-borrow self rn
		unsafe { &mut (*self.repr()).inner }
	}
}

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
		compare_meta::<[i32], [i32; 5]>();
		compare_meta::<dyn Debug, Loud>();
		let ary = [1, 2, 3, 4, 5, 6];
		let boxed: NarrowBox<[i32]> = NarrowBox::new_unsize(ary);
		dbg!(mem::size_of_val(&boxed));
		assert_eq!(mem::size_of_val(&boxed), mem::size_of::<*const ()>());
		dbg!(boxed);
		let err = std::fs::read("/lmao").err().unwrap();
		let boxed: NarrowBox<dyn std::error::Error+'static> = NarrowBox::new_unsize(err);
		dbg!(mem::size_of_val(&boxed));
		dbg!(&boxed);
		dbg!(boxed.downcast_ref::<io::Error>().expect("hmm"));

		let err = std::fs::read("/lmao").err().unwrap();
		let boxed: NarrowBox<io::Error> = NarrowBox::new(err);
		let boxed: NarrowBox<dyn std::error::Error+'static> = boxed.unsize();
		dbg!(boxed.downcast_ref::<io::Error>().expect("hmm"));

		NarrowBox::<dyn Debug>::new_unsize(Loud("neat".to_string()));

		dbg!(mem::size_of::<Repr<dyn Debug, Loud>>());
		dbg!(mem::size_of::<Repr<Loud, Loud>>());

		NarrowBox::new(Loud("sweet".to_string()));
		NarrowBox::new([1, 2, 3, 4]);
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
