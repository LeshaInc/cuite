use std::any::Any;
use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;

slotmap::new_key_type! {
    pub struct NodeId;
}

#[derive(Clone)]
pub struct Node {
    pub value: Option<AnyValue>,
    pub state: NodeState,
    pub kind: NodeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeState {
    Clean,
    Check,
    Dirty,
    DirtyMarked,
}

#[derive(Clone)]
pub enum NodeKind {
    Signal,
    Effect { computation: AnyComputation },
}

pub type AnyValue = Rc<RefCell<dyn Any>>;

pub fn wrap_value<T: 'static>(value: T) -> AnyValue {
    Rc::new(RefCell::new(value))
}

pub trait Computation {
    /// Perform the computation, returning `true` if the value has updated
    fn run(&self, value: AnyValue) -> bool;
}

pub type AnyComputation = Rc<RefCell<dyn Computation>>;

struct EffectComputation<T, F> {
    func: F,
    marker: PhantomData<fn(T) -> T>,
}

impl<T, F> Computation for EffectComputation<T, F>
where
    T: 'static,
    F: 'static + Fn(Option<T>) -> T,
{
    fn run(&self, value: AnyValue) -> bool {
        let old_value = value
            .borrow_mut()
            .downcast_mut::<Option<T>>()
            .unwrap()
            .take();

        let new_value = (self.func)(old_value);

        *value.borrow_mut().downcast_mut::<Option<T>>().unwrap() = Some(new_value);

        true
    }
}

pub fn wrap_effect_computation<T, F>(func: F) -> AnyComputation
where
    T: 'static,
    F: 'static + Fn(Option<T>) -> T,
{
    Rc::new(RefCell::new(EffectComputation {
        func,
        marker: PhantomData,
    }))
}
