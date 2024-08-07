use std::cell::RefCell;

mod host;

pub trait Runtime: 'static {
    fn id(&self) -> u64;
}

thread_local! {
    static RT: RefCell<Option<Box<dyn Runtime>>> = const { RefCell::new(None) };
}

pub fn install_runtime<Ret>(rt: impl Runtime, f: impl FnOnce() -> Ret) -> Ret {
    let old_rt = RT.replace(Some(Box::new(rt)));
    let ret = f();
    RT.set(old_rt);
    ret
}

pub fn with_runtime<Ret>(f: impl FnOnce(&mut dyn Runtime) -> Ret) -> Ret {
    RT.with_borrow_mut(|rt| f(rt.as_deref_mut().unwrap()))
}
