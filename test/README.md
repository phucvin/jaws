cd ..

mkdir wat

cargo build --release

cargo install --locked wasm-tools

./execute.sh test/t01.js 
