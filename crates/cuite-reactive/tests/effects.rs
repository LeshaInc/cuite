use std::cell::RefCell;
use std::rc::Rc;

use cuite_reactive::{create_effect, create_signal};

#[test]
fn simple_effect() {
    let ops: Rc<RefCell<Vec<i32>>> = Default::default();

    {
        let signal = create_signal(0);

        let ops_copy = ops.clone();
        create_effect(move |_| {
            ops_copy.borrow_mut().push(signal.get());
        });

        signal.set(1);
        signal.set(2);
    }

    assert_eq!(ops.borrow().as_slice(), &[0, 1, 2]);
}
