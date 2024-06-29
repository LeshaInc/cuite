use crate::node::{wrap_effect_computation, wrap_value, NodeId};
use crate::runtime::with_runtime;

pub fn create_effect<T, F>(func: F) -> Effect
where
    T: 'static,
    F: 'static + Fn(Option<T>) -> T,
{
    Effect::new(func)
}

#[derive(Debug, Clone, Copy)]
pub struct Effect {
    _id: NodeId,
}

impl Effect {
    pub fn new<T, F>(func: F) -> Effect
    where
        T: 'static,
        F: 'static + Fn(Option<T>) -> T,
    {
        let value = wrap_value(None::<T>);
        let computation = wrap_effect_computation(func);
        let id = with_runtime(|runtime| {
            let id = runtime.create_effect(value, computation);
            runtime.update_if_necessary(id);
            id
        });
        Effect { _id: id }
    }
}
