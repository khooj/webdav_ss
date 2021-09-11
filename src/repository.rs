pub trait Repository: Send {}

pub struct MemoryRepository {}

impl MemoryRepository {
    pub fn new() -> MemoryRepository {
        MemoryRepository {}
    }
}

impl Repository for MemoryRepository {}
