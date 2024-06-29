use std::cell::{Cell, RefCell};
use std::collections::hash_set;

use ahash::AHashSet;
use slotmap::{SecondaryMap, SlotMap};

use crate::node::{AnyComputation, AnyValue, Node, NodeId, NodeKind, NodeState};

pub fn with_runtime<Ret>(func: impl FnOnce(&Runtime) -> Ret) -> Ret {
    thread_local! {
        static RUNTIME: Runtime = Runtime::default();
    }

    RUNTIME.with(func)
}

#[derive(Default)]
pub struct Runtime {
    pub nodes: RefCell<SlotMap<NodeId, Node>>,
    pub node_subscribers: RefCell<SecondaryMap<NodeId, RefCell<AHashSet<NodeId>>>>,
    pub node_sources: RefCell<SecondaryMap<NodeId, RefCell<AHashSet<NodeId>>>>,
    pub observer: Cell<Option<NodeId>>,
    pub pending_effects: RefCell<Vec<NodeId>>,
}

impl Runtime {
    pub fn create_node(&self, node: Node) -> NodeId {
        self.nodes.borrow_mut().insert(node)
    }

    pub fn create_signal(&self, value: AnyValue) -> NodeId {
        self.create_node(Node {
            value: Some(value),
            state: NodeState::Clean,
            kind: NodeKind::Signal,
        })
    }

    pub fn create_effect(&self, value: AnyValue, computation: AnyComputation) -> NodeId {
        self.create_node(Node {
            value: Some(value),
            state: NodeState::Dirty,
            kind: NodeKind::Effect { computation },
        })
    }

    fn get_node_value(&self, id: NodeId) -> Option<AnyValue> {
        let nodes = self.nodes.borrow();
        let node = nodes.get(id)?;
        node.value.clone()
    }

    pub fn with_value<Ret>(
        &self,
        id: NodeId,
        func: impl FnOnce(AnyValue) -> Option<Ret>,
    ) -> Option<Ret> {
        self.get_node_value(id).and_then(func)
    }

    pub fn node_state(&self, id: NodeId) -> NodeState {
        let nodes = self.nodes.borrow();
        match nodes.get(id) {
            Some(node) => node.state,
            _ => NodeState::Clean,
        }
    }

    pub fn mark_clean(&self, id: NodeId) {
        let mut nodes = self.nodes.borrow_mut();
        if let Some(node) = nodes.get_mut(id) {
            node.state = NodeState::Clean;
        }
    }

    pub fn mark_descendants_dirty(&self, root_id: NodeId) {
        let mut nodes = self.nodes.borrow_mut();
        let mut pending_effects = self.pending_effects.borrow_mut();
        let subscribers = self.node_subscribers.borrow();
        let observer = self.observer.get();

        match nodes.get_mut(root_id) {
            Some(node) => {
                if node.state == NodeState::DirtyMarked {
                    return;
                }

                node.state = NodeState::DirtyMarked
            }
            None => return,
        }

        let Some(root_children) = subscribers.get(root_id).map(|v| v.borrow()) else {
            return;
        };

        // DFS using a stack of iterators

        // define a self-referential struct for storing the iterator alongside the borrowed ref
        type Dependent<'a> = hash_set::Iter<'a, NodeId>;
        self_cell::self_cell! {
            struct RefIter<'a> {
                owner: std::cell::Ref<'a, AHashSet<NodeId>>,
                #[not_covariant]
                dependent: Dependent,
            }
        }

        enum Operation<'a> {
            Push(RefIter<'a>),
            Continue,
            Pop,
        }

        let mut stack = Vec::with_capacity(16);
        stack.push(RefIter::new(root_children, |c| c.iter()));

        while let Some(iter) = stack.last_mut() {
            let op = iter.with_dependent_mut(|_, iter| {
                let Some(mut child) = iter.next().copied() else {
                    // no more children: pop the iterator
                    return Operation::Pop;
                };

                loop {
                    let Some(node) = nodes.get_mut(child) else {
                        // node is disposed
                        return Operation::Continue;
                    };

                    if node.state == NodeState::Check || node.state == NodeState::DirtyMarked {
                        // already visited
                        return Operation::Continue;
                    }

                    // mark the node
                    if node.state == NodeState::Clean {
                        node.state = NodeState::Check;
                    }

                    if let NodeKind::Effect { .. } = &node.kind {
                        if observer != Some(child) {
                            pending_effects.push(child)
                        }
                    }

                    let Some(children) = subscribers.get(child).map(|c| c.borrow()) else {
                        // no children
                        return Operation::Continue;
                    };

                    if children.is_empty() {
                        return Operation::Continue;
                    }

                    if children.len() == 1 {
                        // avoid creating an iterator for just 1 child
                        child = *children.iter().next().unwrap();
                        continue;
                    }

                    return Operation::Push(RefIter::new(children, |c| c.iter()));
                }
            });

            match op {
                Operation::Push(iter) => {
                    stack.push(iter);
                }
                Operation::Continue => {
                    continue;
                }
                Operation::Pop => {
                    stack.pop();
                }
            }
        }
    }

    pub fn run_effects(&self) {
        let mut effects = self.pending_effects.take();

        for effect_id in effects.drain(..) {
            self.update_if_necessary(effect_id);
        }

        *self.pending_effects.borrow_mut() = effects;
    }

    pub fn update_if_necessary(&self, node_id: NodeId) {
        if self.node_state(node_id) == NodeState::Check {
            let sources = {
                self.node_sources
                    .borrow()
                    .get(node_id)
                    .map(|v| v.borrow().iter().copied().collect::<Vec<_>>())
            };

            if let Some(sources) = sources {
                for source in sources {
                    self.update_if_necessary(source);

                    if self.node_state(node_id) >= NodeState::Dirty {
                        break;
                    }
                }
            }
        }

        if self.node_state(node_id) >= NodeState::Dirty {
            self.update(node_id);
        }

        self.mark_clean(node_id);
    }

    pub fn with_observer<Ret>(&self, observer: NodeId, func: impl FnOnce() -> Ret) -> Ret {
        let prev_observer = self.observer.take();
        self.observer.set(Some(observer));
        let ret = func();
        self.observer.set(prev_observer);
        ret
    }

    pub fn update(&self, node_id: NodeId) {
        let Some(node) = self.nodes.borrow().get(node_id).cloned() else {
            return;
        };

        let changed = match node.kind {
            NodeKind::Signal => true,
            NodeKind::Effect { computation } => {
                let Some(value) = node.value else { return };

                self.with_observer(node_id, || computation.borrow().run(value))
            }
        };

        if !changed {
            return;
        }

        // mark subscribers dirty
        let subscribers = self.node_subscribers.borrow();
        let mut nodes = self.nodes.borrow_mut();

        let Some(subscribers) = subscribers.get(node_id) else {
            return;
        };

        for child_id in subscribers.borrow().iter() {
            if let Some(node) = nodes.get_mut(*child_id) {
                node.state = NodeState::Dirty;
            }
        }
    }

    pub fn track(&self, node_id: NodeId) {
        let Some(observer) = self.observer.get() else {
            return;
        };

        let mut subscribers = self.node_subscribers.borrow_mut();
        if let Some(subscribers) = subscribers.entry(node_id) {
            subscribers.or_default().borrow_mut().insert(observer);
        }

        let mut sources = self.node_sources.borrow_mut();
        if let Some(sources) = sources.entry(observer) {
            sources.or_default().borrow_mut().insert(node_id);
        }
    }
}
