const fs = require('fs').promises;

async function testWasmGC() {
  try {
    const wasmBuffer = await fs.readFile('./wasm_gc_test.wasm');
    const module = await WebAssembly.compile(wasmBuffer);
    const instance = await WebAssembly.instantiate(module);

    // Try to call a function that uses GC features
    const result = instance.exports.test();
    console.log('WASM GC test result:', result);
    console.log('WASM GC is supported and functional');
  } catch (error) {
    console.error('Error testing WASM GC:', error.message);
    console.log('WASM GC might not be supported or there\'s an issue with the WASM module');
  }
}

async function main() {
  console.log(`Node.js version: ${process.version}`);
  console.log(`V8 version: ${process.versions.v8}\n`);

  await testWasmGC();
}

main().catch(console.error);

console.log("\nNote: Run this script with the WASM GC flag enabled:");
console.log("node --experimental-wasm-gc wasm_gc_test.js");
