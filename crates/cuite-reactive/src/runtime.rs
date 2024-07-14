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

/// Threaded local reactive runtime.
///
/// Manages the reactive nodes (signals, effects, scopes) and their lifetime.
///
/// The nodes have two kinds of relationships:
///
///  - [Source] - [Subscriber]. This forms a DAG of dependencies and updates.
///
///  - Parent - [Child]. This is a forest which determines the lifetime of
///    nodes, their ownership, and handles cleanup.
#[derive(Default)]
pub struct Runtime {
    /// Reactive nodes: signals, effects, scopes, etc
    pub nodes: RefCell<SlotMap<NodeId, Node>>,

    /// Mapping between nodes and their subscribers.
    ///
    /// If node A is a subscriber of node B, then updates of B will also cause
    /// an update of A.
    pub node_subscribers: RefCell<SecondaryMap<NodeId, RefCell<AHashSet<NodeId>>>>,

    /// Mapping between nodes and their sources, i.e. dependencies.
    ///
    /// If A is a subscriber of B, then B is a source of A, and vice versa.
    pub node_sources: RefCell<SecondaryMap<NodeId, RefCell<AHashSet<NodeId>>>>,

    /// Mapping between nodes and their parent. When parent is disposed, all of
    /// the descendants in the hierarchy are also disposed.
    pub node_parents: RefCell<SecondaryMap<NodeId, NodeId>>,

    /// Mapping between nodes and their children.
    ///
    /// Reversed `node_parents` mapping.
    pub node_children: RefCell<SecondaryMap<NodeId, RefCell<AHashSet<NodeId>>>>,

    /// Current scope which will be implicitly assigned as a parent for all
    /// nodes created under it.
    pub scope: Cell<Option<NodeId>>,

    /// Node that is currently being updated.
    ///
    /// When a node is tracked (for example, when getting a signal's value),
    /// that node will be added to the sources of the current observer. And vice
    /// versa: the observer will be added to the subscribers of the tracked
    /// node.
    pub observer: Cell<Option<NodeId>>,

    /// List of effects scheduled to be run during `run_effects`
    pub pending_effects: RefCell<Vec<NodeId>>,
}

impl Runtime {
    /// Creates a node and assigns it to the current scope.
    pub fn create_node(&self, node: Node) -> NodeId {
        let id = self.nodes.borrow_mut().insert(node);

        if let Some(scope) = self.scope.get() {
            self.node_parents.borrow_mut().insert(id, scope);

            let node_children = &mut self.node_children.borrow_mut();
            let children = node_children.entry(id).map(|v| v.or_default());
            if let Some(children) = children {
                children.borrow_mut().insert(id);
            }
        }

        id
    }

    /// Creates a signal with a specified initial value.
    pub fn create_signal(&self, value: AnyValue) -> NodeId {
        self.create_node(Node {
            value: Some(value),
            state: NodeState::Clean,
            kind: NodeKind::Signal,
        })
    }

    /// Creates an effect with a specified initial value and a computation.
    ///
    /// Note that the effect will not be run unless you call
    /// `update_if_necessary`.
    pub fn create_effect(&self, value: AnyValue, computation: AnyComputation) -> NodeId {
        self.create_node(Node {
            value: Some(value),
            state: NodeState::Dirty,
            kind: NodeKind::Effect { computation },
        })
    }

    /// Returns the value of a node, if the node exists and has a value.
    pub fn get_node_value(&self, id: NodeId) -> Option<AnyValue> {
        let nodes = self.nodes.borrow();
        let node = nodes.get(id)?;
        node.value.clone()
    }

    fn node_state(&self, id: NodeId) -> NodeState {
        let nodes = self.nodes.borrow();
        match nodes.get(id) {
            Some(node) => node.state,
            _ => NodeState::Clean,
        }
    }

    fn mark_clean(&self, id: NodeId) {
        let mut nodes = self.nodes.borrow_mut();
        if let Some(node) = nodes.get_mut(id) {
            node.state = NodeState::Clean;
        }
    }

    /// Marks the node dirty. All of its descendants in the subscriber hierarchy
    /// are marked as check, i.e. they will be updated only if one of their
    /// sources actually changes.
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

        // define a self-referential struct for storing the iterator alongside the
        // borrowed ref
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

    /// Runs all the pending effects.
    pub fn run_effects(&self) {
        let mut effects = self.pending_effects.take();

        for effect_id in effects.drain(..) {
            self.update_if_necessary(effect_id);
        }

        *self.pending_effects.borrow_mut() = effects;
    }

    /// Updates the node only if necessary.
    ///
    /// If it's marked as check, the sources will be recursively updated too.
    ///
    /// If it's marked as dirty, it will be updated regardless of the state of
    /// the sources.
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
            self.cleanup_children(node_id);
            self.update(node_id);
        }

        self.mark_clean(node_id);
    }

    /// Runs the given closure in the context of the provided observer.
    ///
    /// For the duration of the closure, `observer` will become the new
    /// `observer` and `scope` as well.
    pub fn with_observer<Ret>(&self, observer: NodeId, func: impl FnOnce() -> Ret) -> Ret {
        let prev_observer = self.observer.take();
        let prev_scope = self.scope.take();

        self.observer.set(Some(observer));
        self.scope.set(Some(observer));

        let ret = func();

        self.observer.set(prev_observer);
        self.scope.set(prev_scope);

        ret
    }

    fn update(&self, node_id: NodeId) {
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

    /// Tracks the given node as a source of the current observer (e.g. a signal
    /// is tracked inside an effect).
    ///
    /// Since the mapping is bidirectional (there's `node_sources` and
    /// `node_subscribers`), both maps will be updated.
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

    fn cleanup_children(&self, node_id: NodeId) {
        let mut children = self.node_children.borrow_mut();
        let Some(children) = children.remove(node_id) else {
            return;
        };

        for child in children.into_inner() {
            self.cleanup_children(child);

            let subscribers = self.node_subscribers.borrow_mut().remove(child);
            if let Some(subscribers) = subscribers {
                for sub in subscribers.into_inner() {
                    if let Some(source) = self.node_sources.borrow_mut().get(sub) {
                        source.borrow_mut().remove(&child);
                    }
                }
            }

            let sources = self.node_sources.borrow_mut().remove(child);
            if let Some(sources) = sources {
                for source in sources.into_inner() {
                    if let Some(sub) = self.node_subscribers.borrow_mut().get(source) {
                        sub.borrow_mut().remove(&child);
                    }
                }
            }

            self.node_parents.borrow_mut().remove(child);
            self.nodes.borrow_mut().remove(child);
        }
    }
}
