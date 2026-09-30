//! Thread-local allocation measurements for explicitly requested benchmarks.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
#[derive(Clone, Copy, Default)]
pub struct Counts {
    pub allocations: u64,
    pub bytes: u64,
}
thread_local! { static COUNTS: Cell<Option<Counts>> = const { Cell::new(None) }; }
struct Counting;
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn record(bytes: usize) {
    let _ = COUNTS.try_with(|slot| {
        if let Some(mut counts) = slot.get() {
            counts.allocations += 1;
            counts.bytes += bytes as u64;
            slot.set(Some(counts));
        }
    });
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        unsafe { System.realloc(pointer, layout, size) }
    }
}
pub fn measure(operation: impl FnOnce()) -> Counts {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            COUNTS.with(|slot| slot.set(None));
        }
    }
    COUNTS.with(|slot| slot.set(Some(Counts::default())));
    let _reset = Reset;
    operation();
    COUNTS.with(|slot| slot.get().unwrap_or_default())
}
