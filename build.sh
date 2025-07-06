
#./build.sh test就会运行测试指令
#./build.sh build就会运行构建指令

if [ "$1" = "test" ]; then
    RUST_BACKTRACE=full cargo test -- --test-threads=1 --nocapture
elif [ "$1" = "build" ]; then
    cargo build --target=thumbv7em-none-eabihf
else
    echo "Usage: ./build.sh [test|build]"
    exit 1
fi