#![feature(extern_types, ptr_metadata, unsize, option_result_unwrap_unchecked)]

use std::fmt;
use std::mem;
use std::ptr::{self, Pointee};
use std::marker::{Unsize, PhantomData};
use std::ops::{Deref, DerefMut};
use std::fmt::Debug;

#[repr(transparent)] // <- i forgot if i want this
struct NarrowBox<Dyn: ?Sized>(ptr::NonNull<Opaque<Dyn>>);

#[repr(C)]
struct WrapUnsized<Dyn: ?Sized, T> {
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

// WrapUnsized's metadata for DynWithDrop effectively encodes
// Dyn's metadata and its layout.
trait DynWithDrop<Dyn: ?Sized> {
    fn inner(&mut self) -> *mut Dyn;
}

impl<Dyn: ?Sized, T: Unsize<Dyn>> DynWithDrop<Dyn> for WrapUnsized<Dyn, T> {
    fn inner(&mut self) -> *mut Dyn {
        // we're unsizing from T to Dyn here
        ptr::addr_of_mut!(self.inner)
    }
}

impl<Dyn: ?Sized> NarrowBox<Dyn> {
    fn new_unsize<T: Unsize<Dyn>>(inner: T) -> NarrowBox<Dyn> {
        // synthesize vtable for thing we dont have yet.
        // i'm sure this is safe.
        let normal_ptr: *const WrapUnsized<Dyn, T> = ptr::null();
        let wide_ptr: *const dyn DynWithDrop<Dyn> = normal_ptr;
        let wide_ptr: *const (dyn DynWithDrop<Dyn>+'static) =
            // Safety: metadata is probably the same regardless of lifetime
            unsafe { mem::transmute(wide_ptr) }; // close enough

        let metadata = ptr::metadata(wide_ptr);
        let _phantom = PhantomData;

        let transparent: WrapUnsized<Dyn, T> =
            WrapUnsized { metadata, inner, _phantom };
        let boxed = Box::new(transparent);
        let transparent_metadata = ptr::addr_of!(boxed.metadata);
        let opaque = Box::into_raw(boxed) as *mut Opaque<Dyn>;

        unsafe {
            // safety: i've done this in C for years, it must be fine
            debug_assert_eq!(transparent_metadata as usize,
                ptr::addr_of!((*opaque).metadata) as usize);
        }

        // safety: we just allocated it
        unsafe { NarrowBox(ptr::NonNull::new_unchecked(opaque)) }
    }

    fn new(inner: Dyn) -> NarrowBox<Dyn> where Dyn: Sized {
        assert!(Self::is_sized());
        let boxed: *mut Dyn = Box::into_raw(Box::new(inner));
        let opaque = boxed as *mut Opaque<Dyn>;

        // safety: we just allocated it
        unsafe { NarrowBox(ptr::NonNull::new_unchecked(opaque)) }
    }

    fn is_sized() -> bool {
        mem::size_of::<<Dyn as Pointee>::Metadata>() == 0
    }

    // only call if is_sized()
    fn as_ptr(&self) -> *mut Dyn {
        if Self::is_sized() {
            let p = self.0.as_ptr();
            // safety: it's the Box<Dyn> from back in new()
            unsafe { mem::transmute_copy(&p) }
        } else {
            // safety: as_dyn_with_drop wouldn't do that to us
            unsafe { (*self.as_dyn_with_drop()).inner() }
        }
    }

    // only call if not is_sized()
    fn as_dyn_with_drop(&self) -> *mut dyn DynWithDrop<Dyn> {
        assert!(!Self::is_sized());

        let p = self.0.as_ptr();
        // safety: p points at the WrapUnsized we created back in new_unsize,
        // Opaque's metadata field is at the same address as WrapUnsized's,
        // so they're the same and it's safe to grab
        let metadata = unsafe { (*p).metadata };
        ptr::from_raw_parts_mut(p as *mut (), metadata)
    }
}

impl<Dyn: ?Sized> Drop for NarrowBox<Dyn> {
    fn drop(&mut self) {
        if Self::is_sized() {
            // safety: it's our Box from new
            unsafe { Box::<Dyn>::from_raw(self.as_ptr()); }
        } else {
            unsafe {
                let d = self.as_dyn_with_drop();
                // safety: it's the Box from new_unsize
                Box::from_raw(d);
            }
        }
    }
}

impl<Dyn: ?Sized> Deref for NarrowBox<Dyn> {
    type Target = Dyn;
    fn deref(&self) -> &Dyn {
        // safety: yes
        unsafe { &*self.as_ptr() }
    }
}

impl<Dyn: ?Sized> DerefMut for NarrowBox<Dyn> {
    fn deref_mut(&mut self) -> &mut Dyn {
        // safety: as_ptr doesnt return null, also we mut-borrow self rn
        unsafe { &mut *self.as_ptr() }
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

fn main() {
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
