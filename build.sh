
#./build.sh test就会运行测试指令
#./build.sh build就会运行构建指令

if [ "$1" = "test" ]; then
    # RUST_BACKTRACE=full cargo test -- --test-threads=1 --nocapture
    RUSTFLAGS='--cfg test' cargo test --lib -- --test-threads=1
elif [ "$1" = "build" ]; then
    cd examples/cortex-m3
    cargo build --target=thumbv7m-none-eabi
elif [ "$1" = "run" ]; then
    cd examples/cortex-m3
    cargo build --target=thumbv7m-none-eabi

    #运行cortex-m3的例子
    qemu-system-arm \
    -cpu cortex-m3 \
    -machine lm3s6965evb \
    -nographic \
    -semihosting-config enable=on,target=native \
    -kernel target/thumbv7m-none-eabi/debug/neon-rtos2-example-cortex-m3 

else
    echo "Usage: ./build.sh [test|build]"
    exit 1
fi