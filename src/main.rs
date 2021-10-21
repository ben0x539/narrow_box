#![feature(extern_types, ptr_metadata, unsize)]

use std::fmt;
use std::mem;
use std::ptr::{self, Pointee};
use std::marker::{Unsize, PhantomData};
use std::ops::{Deref, DerefMut};
use std::fmt::Debug;

#[repr(transparent)] // <- i forgot if i want this
struct NarrowBox<Dyn: ?Sized>(ptr::NonNull<Opaque<Dyn>>);

#[repr(C)]
struct WrapUnsized<Dyn: ?Sized, T: ?Sized> {
    metadata: <Dyn as Pointee>::Metadata,
    inner: T,
}

#[repr(C)]
struct Opaque<Dyn: ?Sized> {
    metadata: <Dyn as Pointee>::Metadata,
    _phantom: PhantomData<Dyn>,
    _extern: Extern, // idk if we even need this?
}

extern { type Extern; }

fn synthesize_metadata<Dyn: ?Sized, T: Unsize<Dyn>>()
        -> <Dyn as Pointee>::Metadata {
    let narrow_dummy: *const T = ptr::null();
    let wide_dummy: *const Dyn = narrow_dummy;
    ptr::metadata(wide_dummy)
}

impl<Dyn: ?Sized> NarrowBox<Dyn> {
    fn new_unsize<T>(inner: T) -> NarrowBox<Dyn> where T: Unsize<Dyn> {
        let metadata = synthesize_metadata::<Dyn, T>();
        unsafe { Self::new_with_meta(inner, metadata) }
    }

    fn new(inner: Dyn) -> NarrowBox<Dyn> where Dyn: Sized {
        let metadata = ptr::metadata::<Dyn>(ptr::null());
        unsafe { Self::new_with_meta(inner, metadata) }
    }

    // must be the right metadata
    unsafe fn new_with_meta<T>(inner: T, metadata: <Dyn as Pointee>::Metadata)
            -> NarrowBox<Dyn> {
        let boxed: Box<WrapUnsized<Dyn, T>> =
            Box::new(WrapUnsized { metadata, inner });
        let opaque = Box::into_raw(boxed) as *mut Opaque<Dyn>;

        // safety: we just allocated it
        NarrowBox(ptr::NonNull::new_unchecked(opaque))
    }

    fn wrapped(&self) -> *mut WrapUnsized<Dyn, Dyn> {
        let p = self.0.as_ptr();
        ptr::from_raw_parts_mut(
            p as *mut (),
            // safety: HOPEFULLY, i didnt read the rfc yet
            unsafe { mem::transmute_copy(&(*p).metadata) })
    }

    fn inner(&self) -> *mut Dyn {
        unsafe { &mut (*self.wrapped()).inner }
    }
}

impl<Dyn: ?Sized> Drop for NarrowBox<Dyn> {
    fn drop(&mut self) {
        unsafe { Box::from_raw(self.wrapped()); }
    }
}

impl<Dyn: ?Sized> Deref for NarrowBox<Dyn> {
    type Target = Dyn;
    fn deref(&self) -> &Dyn {
        // safety: yes
        unsafe { &*self.inner() }
    }
}

impl<Dyn: ?Sized> DerefMut for NarrowBox<Dyn> {
    fn deref_mut(&mut self) -> &mut Dyn {
        // safety: inner doesnt return null, also we mut-borrow self rn
        unsafe { &mut *self.inner() }
    }
}

impl<Dyn: ?Sized+Debug> Debug for NarrowBox<Dyn> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.deref().fmt(f)
    }
}

// ...

#[derive(Debug)]
#[repr(align(1024))]
struct Loud(String);

impl Drop for Loud {
    fn drop(&mut self) {
        println!("dropping {:?}", self);
        assert!(self.0 != "oh no");
        self.0 = "oh no".to_string();
    }
}

fn compare_meta<Dyn: ?Sized, T: Unsize<Dyn>>() {
    unsafe {
        let m1: usize = mem::transmute_copy(&synthesize_metadata::<Dyn, T>());
        let m2: usize = mem::transmute_copy(&synthesize_metadata::<WrapUnsized<Dyn, Dyn>, WrapUnsized<Dyn, T>>());
        assert_eq!(m1, m2);
    }
}

fn main() {
    compare_meta::<[i32], [i32; 5]>();
    compare_meta::<dyn Debug, Loud>();
    let ary = [1, 2, 3, 4, 5, 6];
    let boxed: NarrowBox<[i32]> = NarrowBox::new_unsize(ary);
    println!("{}", mem::size_of_val(&boxed));
    println!("{:?}", boxed);
    let err = std::fs::read("/lmao").err().unwrap();
    let boxed: NarrowBox<dyn std::error::Error> = NarrowBox::new_unsize(err);
    println!("{}", mem::size_of_val(&boxed));
    println!("{:?}", boxed);
    NarrowBox::<dyn Debug>::new_unsize(Loud("neat".to_string()));

    NarrowBox::new(Loud("sweet".to_string()));
    let boxed = NarrowBox::new([1, 2, 3, 4]);
    println!("{:?}", boxed);
}
