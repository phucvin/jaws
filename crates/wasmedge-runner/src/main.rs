use futures::prelude::*;
use futures::stream::FuturesUnordered;
use std::{collections::HashMap, fs, future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use wasmedge_sdk::{
    config::{
        CommonConfigOptions, Config, ConfigBuilder, RuntimeConfigOptions, StatisticsConfigOptions,
    },
    error::CoreError,
    r#async::{import::ImportObjectBuilder, vm::Vm, AsyncInstance},
    CallingFrame, Module, Store, WasmValue,
};

// Shared state for our runtime
#[derive(Default)]
struct RuntimeState {
    pollables: Vec<Pin<Box<dyn futures::future::Future<Output = i32> + Send>>>,
    pollable_index: i32,
}

fn poll_handler(
    state: &mut Arc<Mutex<RuntimeState>>,
    _inst: &mut AsyncInstance,
    _frame: &mut CallingFrame,
    inputs: Vec<WasmValue>,
) -> Box<dyn Future<Output = Result<Vec<WasmValue>, CoreError>> + Send> {
    let state = state.clone();
    Box::new(async move {
        if inputs.len() != 3 {
            return Err(CoreError::Execution(
                wasmedge_sdk::error::CoreExecutionError::CastFailed,
            ));
        }

        let ptr = inputs[0].to_i32();
        let length = inputs[1].to_i32();
        let return_ptr = inputs[2].to_i32();

        println!(
            "poll called with ptr={}, length={}, return_ptr={}",
            ptr, length, return_ptr
        );

        let mut locked = state.lock().await;
        // For now just wait for the first pollable to complete
        if let Some(handle) = locked.pollables.first_mut() {
            let result = handle.await;
            println!("Pollable completed with result: {}", result);
        }

        Ok(vec![WasmValue::from_i32(0)])
    })
}

fn subscribe_duration(
    state: &mut Arc<Mutex<RuntimeState>>,
    _inst: &mut AsyncInstance,
    _frame: &mut CallingFrame,
    inputs: Vec<WasmValue>,
) -> Box<dyn Future<Output = Result<Vec<WasmValue>, CoreError>> + Send> {
    let state = state.clone();
    Box::new(async move {
        println!("foo");
        if inputs.is_empty() {
            return Err(CoreError::Execution(
                wasmedge_sdk::error::CoreExecutionError::CastFailed,
            ));
        }

        let duration_nanos = inputs[0].to_i64();
        let duration_millis = duration_nanos / 1_000_000;
        let index = { state.lock().await.pollable_index };

        println!(
            "Creating timer for {}ms with index {}",
            duration_millis, index
        );

        let handle = async move {
            tokio::time::sleep(Duration::from_millis(duration_millis as u64)).await;
            index
        };

        {
            let mut locked = state.lock().await;
            locked.pollables.push(Box::pin(handle));
            locked.pollable_index += 1;
        }

        Ok(vec![WasmValue::from_i32(index)])
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(Mutex::new(RuntimeState::default()));
    let common_options = CommonConfigOptions::default()
        .bulk_memory_operations(true)
        .multi_value(true)
        .mutable_globals(true)
        .non_trap_conversions(true)
        .reference_types(true)
        .sign_extension_operators(true)
        .gc(true)
        .simd(true);

    let stat_options = StatisticsConfigOptions::default()
        .count_instructions(true)
        .measure_cost(true)
        .measure_time(true);

    let runtime_options = RuntimeConfigOptions::default().max_memory_pages(1024);

    let mut config = ConfigBuilder::new(common_options)
        .with_statistics_config(stat_options)
        .with_runtime_config(runtime_options)
        .build()
        .unwrap();
    //
    // Create import objects for WASI preview 2
    let mut poll_builder = ImportObjectBuilder::new("wasi:io/poll@0.2.1", state.clone()).unwrap();
    poll_builder
        .with_func::<(i32, i32, i32), i32>("poll", poll_handler)
        .unwrap();
    let mut poll_import = poll_builder.build();

    let mut clock_builder =
        ImportObjectBuilder::new("wasi:clocks/monotonic-clock@0.2.1", state.clone()).unwrap();
    clock_builder
        .with_func::<i64, i32>("subscribe-duration", subscribe_duration)
        .unwrap();
    let mut clock_import = clock_builder.build();

    // Set up the WasmEdge VM
    let mut instances = HashMap::new();
    instances.insert("wasi:io/poll@0.2.1".into(), &mut poll_import);
    instances.insert(
        "wasi:clocks/monotonic-clock@0.2.1".into(),
        &mut clock_import,
    );

    let store = Store::new(Some(&config), instances).unwrap();
    let mut vm = Vm::new(store);

    let module = Module::from_file(None, "wasm/generated.wasm").unwrap();
    vm.register_module(None, module).unwrap();
    vm.run_func(None, "run", []).await?;

    Ok(())
}
