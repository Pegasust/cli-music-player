use enum_dispatch::enum_dispatch;
use serde_json::Value;

#[enum_dispatch]
pub trait Factory<T> {
    fn generate(&self, args: Value) -> T;
}


