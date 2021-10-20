#![feature(extern_types, ptr_metadata, unsize, option_result_unwrap_unchecked)]

use std::fmt;
use std::ptr::{self, Pointee};
use std::marker::{Unsize, PhantomData};
use std::ops::{Deref, DerefMut};
use std::fmt::Debug;

#[repr(transparent)] // <- i forgot if i want this
struct NarrowBox<Dyn: ?Sized>(ptr::NonNull<Opaque<Dyn>>);

#[repr(C)]
struct Transparent<Dyn: ?Sized, T: Unsize<Dyn>> {
    metadata: <dyn DynWithDrop<Dyn> as Pointee>::Metadata,
    inner: T,
    _phantom: PhantomData<Dyn>,
}

#[repr(C)]
struct Opaque<Dyn: ?Sized> {
    metadata: <dyn DynWithDrop<Dyn> as Pointee>::Metadata,
    _phantom: PhantomData<Dyn>,
    _extern: Extern, // idk if we even need this?
}

extern { type Extern; }

trait DynWithDrop<Dyn: ?Sized> {
    unsafe fn inner(&self) -> *const Dyn;
    unsafe fn drop_as_box(&mut self);
}

impl<Dyn: ?Sized, T: Unsize<Dyn>> DynWithDrop<Dyn> for Transparent<Dyn, T> {
    unsafe fn inner(&self) -> *const Dyn {
        ptr::addr_of!(self.inner) as *const Dyn
    }

    unsafe fn drop_as_box(&mut self) {
        drop(Box::from_raw(self as *mut Self));
    }
}

impl<Dyn: ?Sized> NarrowBox<Dyn> {
    fn new<T: Unsize<Dyn>>(inner: T) -> NarrowBox<Dyn> {
        // synthesize vtable for thing we dont have yet.
        // i'm sure this is safe.
        let normal_ptr: *const Transparent<Dyn, T> = ptr::null();
        let wide_ptr: *const dyn DynWithDrop<Dyn> = normal_ptr;
        let wide_ptr: *const (dyn DynWithDrop<Dyn>+'static) =
            unsafe { mem::transmute(wide_ptr) }; // todo: ugh
        let metadata = ptr::metadata(wide_ptr);
        let _phantom = PhantomData;

        let transparent: Transparent<Dyn, T> =
            Transparent { metadata, inner, _phantom };
        let boxed: *mut Transparent<Dyn, T> =
            Box::into_raw(Box::new(transparent));
        let opaque = boxed as *mut Opaque<Dyn>;

        unsafe { NarrowBox(ptr::NonNull::new_unchecked(opaque)) }
    }

    fn rebuild(&self) -> *const dyn DynWithDrop<Dyn> {
        unsafe {
            let p = self.0.as_ptr();
            ptr::from_raw_parts(p as *const (), (*p).metadata)
        }
    }

    fn rebuild_mut(&self) -> *mut dyn DynWithDrop<Dyn> {
        unsafe {
            let p = self.0.as_ptr();
            ptr::from_raw_parts_mut(p as *mut (), (*p).metadata)
        }
    }
}

impl<Dyn: ?Sized> Drop for NarrowBox<Dyn> {
    fn drop(&mut self) {
        unsafe { (&mut *self.rebuild_mut()).drop_as_box() }
    }
}

impl<Dyn: ?Sized> Deref for NarrowBox<Dyn> {
    type Target = Dyn;
    fn deref(&self) -> &Dyn {
        unsafe { &*(&*self.rebuild()).inner() }
    }
}

impl<Dyn: ?Sized> DerefMut for NarrowBox<Dyn> {
    fn deref_mut(&mut self) -> &mut Dyn {
        unsafe { &mut *((&*self.rebuild_mut()).inner() as *mut _) }
    }
}

impl<Dyn: ?Sized+Debug> Debug for NarrowBox<Dyn> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        (**self).fmt(f)
    }
}

// ...

use std::mem;

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

fn main() {
    let ary = [1, 2, 3, 4, 5, 6];
    let boxed: NarrowBox<[i32]> = NarrowBox::new(ary);
    println!("{}", mem::size_of_val(&boxed));
    println!("{:?}", boxed);
    let err = std::fs::read("/lmao").err().unwrap();
    let boxed: NarrowBox<dyn std::error::Error> = NarrowBox::new(err);
    println!("{}", mem::size_of_val(&boxed));
    println!("{:?}", boxed);
    NarrowBox::<dyn Debug>::new(Loud("neat".to_string()));
}
