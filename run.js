const tagToImport = new WebAssembly.Tag({ parameters: ["i32", "i64"] });

let instance;
const importObject = {
  m: {
    t: tagToImport,
  },

  "wasi:io/poll@0.2.1": {
    "poll": function() {},
  },
  "wasi:clocks/monotonic-clock@0.2.1": {
    "subscribe-duration": function() {}
  },
  console: { log: (value) => console.log(`WebAssembly log: ${value}`) },
  "wasi_snapshot_preview1": {
    proc_exit(code) {
      console.log("exit code: ", code);
    },
    fd_write(fd, iovsPtr, iovsLength, bytesWrittenPtr) {
      const iovs = new Uint32Array(instance.exports.memory.buffer, iovsPtr, iovsLength * 2);
      if (fd === 1) { //stdout
          let totalBytesWritten = 0;
          let chunk = "";
          
          for (let i = 0; i < iovsLength * 2; i += 2) {
              const offset = iovs[i];
              const length = iovs[i + 1];
              const bytes = new Int8Array(instance.exports.memory.buffer, offset, length);
              
              for (let j = 0; j < length; j++) {
                  chunk += String.fromCharCode(bytes[j]);
              }
              
              totalBytesWritten += length;
          }
          
          const dataView = new DataView(instance.exports.memory.buffer);
          dataView.setInt32(bytesWrittenPtr, totalBytesWritten, true);
          write(chunk);
      }
      return 0;
    }
  }
};

(async function() {
  // Load and instantiate the WebAssembly module
  // let response = await fetch('tests/ref-cast.wasm');
  // let response = await fetch('wasm/generated.wasm');
  const bytes = read('wasm/generated.optimized.wasm', 'binary');
  // let bytes = await response.arrayBuffer();
  let compiled = await WebAssembly.compile(bytes, { builtins: ['js-string'] });
  instance = await WebAssembly.instantiate(compiled, importObject)
  const exports = instance.exports;
  //     
  // // Call the start function
  console.time('start');
  let result = exports["wasi:cli/run@0.2.1#run"]();
//  console.log("result: ", result);
  console.timeEnd('start');

  // }).catch(error => {
  //   debugger
  //     log('Error: ' + error);
  // });
})();
