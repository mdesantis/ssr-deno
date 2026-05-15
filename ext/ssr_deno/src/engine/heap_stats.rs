use deno_runtime::worker::MainWorker;
use serde::Serialize;

use super::SSRDenoError;

// ---------------------------------------------------------------------------
// V8 heap statistics
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct HeapStats {
    total_heap_size: usize,
    total_heap_size_executable: usize,
    total_physical_size: usize,
    total_available_size: usize,
    used_heap_size: usize,
    heap_size_limit: usize,
    malloced_memory: usize,
    external_memory: usize,
    peak_malloced_memory: usize,
    number_of_native_contexts: usize,
    number_of_detached_contexts: usize,
    total_global_handles_size: usize,
    used_global_handles_size: usize,
}

pub fn collect_heap_stats(worker: &mut MainWorker) -> Result<String, SSRDenoError> {
    let js_runtime = &mut worker.js_runtime;
    let isolate = js_runtime.v8_isolate();
    let stats = isolate.get_heap_statistics();

    let heap = HeapStats {
        total_heap_size: stats.total_heap_size(),
        total_heap_size_executable: stats.total_heap_size_executable(),
        total_physical_size: stats.total_physical_size(),
        total_available_size: stats.total_available_size(),
        used_heap_size: stats.used_heap_size(),
        heap_size_limit: stats.heap_size_limit(),
        malloced_memory: stats.malloced_memory(),
        external_memory: stats.external_memory(),
        peak_malloced_memory: stats.peak_malloced_memory(),
        number_of_native_contexts: stats.number_of_native_contexts(),
        number_of_detached_contexts: stats.number_of_detached_contexts(),
        total_global_handles_size: stats.total_global_handles_size(),
        used_global_handles_size: stats.used_global_handles_size(),
    };

    serde_json::to_string(&heap).map_err(|e| {
        SSRDenoError::HeapStatsSerialization(format!("Failed to serialize heap stats: {e}"))
    })
}
