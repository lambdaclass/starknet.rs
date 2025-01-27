run_bench() {
    local OUTPUT=$(${*:2})
    local NUM_EXECS=$(echo $OUTPUT | cut -d' ' -f3)
    local TOTAL_TIME=$(echo $OUTPUT | cut -d' ' -f6)
    local EXEC_SPEED=$(echo $OUTPUT | cut -d' ' -f8 | cut -c2-)
    local EXEC_TIME=$(echo $OUTPUT | cut -d' ' -f11)

    if [[ -z "$NUM_EXECS" || -z "$TOTAL_TIME" || -z "$EXEC_SPEED" || -z "$EXEC_TIME" ]]; then
        echo "| $1 | CRASHED | CRASHED | CRASHED | CRASHED |"
    else
        echo "| $1 | $NUM_EXECS | $TOTAL_TIME | $EXEC_SPEED | $EXEC_TIME |"
    fi
}

# Clone cairo native and build the runtime.
git clone https://github.com/lambdaclass/cairo_native.git cairo-native
cd cairo-native/
cargo build --release --package cairo-native-runtime
cd ..

# Run the benches.
export CAIRO_NATIVE_RUNTIME_LIBDIR="$(pwd)/cairo-native/target/release"
BENCH_VM=$(run_bench "VM"         cargo bench --bench yas)
BENCH_JIT=$(run_bench "Native JIT" cargo bench --bench yas --features=cairo-native -- jit)
BENCH_AOT=$(run_bench "Native AOT" cargo bench --bench yas --features=cairo-native -- aot)

# Write the results.
echo "# Benchmarking results" > bench-yas.md
echo "" >> bench-yas.md
echo "| Name | Number of runs | Total time (s) | Speed (#/s) | Individual time (s/#) |" >> bench-yas.md
echo "|------|----------------|----------------|-------------|-----------------------|" >> bench-yas.md
echo $BENCH_VM >> bench-yas.md
echo $BENCH_JIT >> bench-yas.md
echo $BENCH_AOT >> bench-yas.md
