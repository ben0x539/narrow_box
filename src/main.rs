#![feature(extern_types, ptr_metadata, unsize, coerce_unsized)]

use std::fmt;
use std::fmt::Debug;
use std::marker::{PhantomData, Unsize};
use std::mem;
use std::ops::{CoerceUnsized, Deref, DerefMut};
use std::ptr::{self, Pointee};

#[repr(transparent)] // <- i forgot if i want this
struct NarrowBox<Dyn: ?Sized>(ptr::NonNull<Opaque<Dyn>>);

#[repr(C)]
struct WrapUnsized<Dyn: ?Sized, T: ?Sized> {
    metadata: WrapMeta<Dyn>,
    inner: T,
}
type WrapMeta<T> = <WrapUnsized<T, T> as Pointee>::Metadata;

impl<Dyn: ?Sized, T> CoerceUnsized<WrapUnsized<Dyn, Dyn>>
    for WrapUnsized<Dyn, T>
    where T: CoerceUnsized<Dyn> {}

#[repr(C)]
struct Opaque<Dyn: ?Sized> {
    metadata: WrapMeta<Dyn>,
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
        let metadata = synthesize_metadata::<
            WrapUnsized<Dyn, Dyn>, WrapUnsized<Dyn, T>>();
        unsafe { Self::new_with_meta(inner, metadata) }
    }

    fn new(inner: Dyn) -> NarrowBox<Dyn> where Dyn: Sized {
        let metadata = ptr::metadata::<WrapUnsized<Dyn, Dyn>>(ptr::null());
        unsafe { Self::new_with_meta(inner, metadata) }
    }

    pub fn into_inner(self) -> Dyn where Dyn: Sized {
        let self_ = mem::ManuallyDrop::new(self);
        // safety: how could this go wrong
        let boxed = unsafe { Box::from_raw(self_.wrapped()) };
        boxed.inner
    }

    // must be the right metadata
    unsafe fn new_with_meta<T>(inner: T, metadata: WrapMeta<Dyn>)
            -> NarrowBox<Dyn> {
        let boxed = Box::new(WrapUnsized { metadata, inner });
        let opaque = Box::into_raw(boxed) as *mut Opaque<Dyn>;

        NarrowBox(ptr::NonNull::new(opaque).unwrap())
    }

    fn wrapped(&self) -> *mut WrapUnsized<Dyn, Dyn> {
        unsafe {
            let p = self.0.as_ptr();
            ptr::from_raw_parts_mut(p as *mut (), (*p).metadata)
        }
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

impl<Dyn: ?Sized + Debug> Debug for NarrowBox<Dyn> {
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
        eprintln!("dropping {:?}", self);
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
    dbg!(mem::size_of_val(&boxed));
    dbg!(boxed);
    let err = std::fs::read("/lmao").err().unwrap();
    let boxed: NarrowBox<dyn std::error::Error> = NarrowBox::new_unsize(err);
    dbg!(mem::size_of_val(&boxed));
    dbg!(boxed);
    NarrowBox::<dyn Debug>::new_unsize(Loud("neat".to_string()));

    dbg!(mem::size_of::<WrapUnsized<dyn Debug, Loud>>());
    dbg!(mem::size_of::<WrapUnsized<Loud, Loud>>());

    NarrowBox::new(Loud("sweet".to_string()));
    NarrowBox::new([1, 2, 3, 4]);
    NarrowBox::new(Loud("ok!".to_string())).into_inner();
}
