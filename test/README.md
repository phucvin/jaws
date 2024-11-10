cd ..

mkdir wat

mkdir wasm

nvm install 23

cargo build --release

cargo install --locked wasm-tools

./execute.sh test/t01.js 

> 678

cp wat/generated.wat test/t01.wat

```
$ time ./execute.sh test/fib.js
102334155

real    0m24.469s
```
