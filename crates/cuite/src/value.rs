use std::any::TypeId;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::mem::offset_of;
use std::ptr::NonNull;

use crate::runtime::with_runtime;

pub struct AnyValue {
    inner: NonNull<u8>,
}

struct Header {
    runtime_id: u64,
    type_hash: u64,
    dtor: unsafe fn(*mut u8),
}

#[repr(C)]
struct Inner<T> {
    header: Header,
    value: T,
}

impl AnyValue {
    pub fn new<T: 'static>(value: T) -> AnyValue {
        let inner = Box::into_raw(Box::new(Inner {
            header: Header {
                runtime_id: get_runtime_id(),
                type_hash: get_type_hash::<T>(),
                dtor: |ptr| {
                    // SAFETY: caller guarantees that ptr is valid
                    unsafe { drop(Box::from_raw(ptr as *mut Inner<T>)) };
                },
            },
            value,
        }));

        // SAFETY: Box::into_raw guarantees that ptr is non null
        let inner = unsafe { NonNull::new_unchecked(inner).cast() };

        AnyValue { inner }
    }

    pub fn downcast<T: 'static>(self) -> T {
        let header = self.inner.as_ptr() as *mut Header;

        // SAFETY: header is a valid pointer as per AnyValue invariant
        let runtime_id = unsafe { (*header).runtime_id };
        let type_hash = unsafe { (*header).type_hash };

        assert_eq!(
            runtime_id,
            get_runtime_id(),
            "runtime id mismatch: attempt to use downcast a value created within a different runtime"
        );

        assert_eq!(
            type_hash,
            get_type_hash::<T>(),
            "type id mismatch: expected a {}",
            std::any::type_name::<T>(),
        );

        // SAFETY: we've checked for type equality
        unsafe {
            let offset = offset_of!(Inner<T>, value);
            let value = (header as *mut u8).add(offset) as *mut T;
            std::ptr::read(value)
        }
    }
}

impl Drop for AnyValue {
    fn drop(&mut self) {
        let header = self.inner.as_ptr() as *mut Header;

        // SAFETY: header is a valid pointer as per AnyValue invariant
        unsafe { ((*header).dtor)(header as *mut u8) }
    }
}

fn get_runtime_id() -> u64 {
    with_runtime(|rt| rt.id())
}

fn get_type_hash<T: 'static>() -> u64 {
    let mut hasher = DefaultHasher::default();
    TypeId::of::<T>().hash(&mut hasher);
    hasher.finish()
}
