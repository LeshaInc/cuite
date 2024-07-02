use std::fmt;
use std::marker::PhantomData;

use super::node::{wrap_value, NodeId};
use super::runtime::with_runtime;

pub fn create_signal<T: 'static>(value: T) -> Signal<T> {
    Signal::new(value)
}

pub struct Signal<T> {
    id: NodeId,
    marker: PhantomData<T>,
}

impl<T: 'static> Signal<T> {
    pub fn new(value: T) -> Signal<T> {
        let value = wrap_value(value);
        let id = with_runtime(|runtime| runtime.create_signal(value));
        Signal {
            id,
            marker: PhantomData,
        }
    }

    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.track();
        self.get_untracked()
    }

    pub fn get_untracked(&self) -> T
    where
        T: Clone,
    {
        self.with_untracked(T::clone)
    }

    pub fn with<Ret>(&self, func: impl FnOnce(&T) -> Ret) -> Ret {
        self.track();
        self.with_untracked(func)
    }

    pub fn with_untracked<Ret>(&self, func: impl FnOnce(&T) -> Ret) -> Ret {
        with_runtime(|runtime| {
            let value = runtime.get_node_value(self.id)?;
            let borrow = value.borrow();
            let casted = borrow.downcast_ref::<T>()?;
            Some(func(casted))
        })
        .unwrap()
    }

    pub fn track(&self) {
        with_runtime(|runtime| runtime.track(self.id));
    }

    pub fn set(&self, value: T) -> T {
        self.update(|v| std::mem::replace(v, value))
    }

    pub fn set_untracked(&self, value: T) -> T {
        self.update_untracked(|v| std::mem::replace(v, value))
    }

    pub fn update<Ret>(&self, func: impl FnOnce(&mut T) -> Ret) -> Ret {
        with_runtime(|runtime| {
            let ret = {
                let value = runtime.get_node_value(self.id)?;
                let mut borrow = value.borrow_mut();
                let casted = borrow.downcast_mut::<T>()?;
                func(casted)
            };

            runtime.mark_descendants_dirty(self.id);
            runtime.run_effects();

            Some(ret)
        })
        .unwrap()
    }

    pub fn update_untracked<Ret>(&self, func: impl FnOnce(&mut T) -> Ret) -> Ret {
        with_runtime(|runtime| {
            let value = runtime.get_node_value(self.id)?;
            let mut borrow = value.borrow_mut();
            let casted = borrow.downcast_mut::<T>()?;
            Some(func(casted))
        })
        .unwrap()
    }
}

impl<T> fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signal({})", std::any::type_name::<T>())
    }
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Signal<T> {}
