cd ..

mkdir wat

mkdir wasm

cargo build --release

cargo install --locked wasm-tools

./execute.sh test/t01.js 

cp wat/generated.wat test/t01.wat
