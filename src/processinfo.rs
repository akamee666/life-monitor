#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub name: String,
    pub time_spent: u64,
    pub instance: String,
    pub window_class: String,
}
