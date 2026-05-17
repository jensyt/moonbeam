#!/bin/bash

# Port to run benchmarks on
PORT=3030
# Duration for each benchmark
DURATION=5s
# Threads for wrk
WRK_THREADS=4
# Connections for wrk
WRK_CONNECTIONS=100

echo "Building benchmarks..."
cargo build --release --manifest-path benchmarks/moonbeam/Cargo.toml
cargo build --release --manifest-path benchmarks/rouille/Cargo.toml
cargo build --release --manifest-path benchmarks/tokio/Cargo.toml

function run_bench {
	local name=$1
	local command=$2
	local url_path=$3
	echo "--------------------------------------------------"
	echo "Benchmarking: $name"
	
	if [[ $command == ./* ]]; then
		if [ ! -f $command ]; then
			echo "Skipping $name: binary not found ($command)."
			return
		fi
	fi

	echo "Command: $command"

	# Start the server in the background
	$command > /dev/null 2>&1 &
	SERVER_PID=$!

	# Give it a second to start
	sleep 2

	# Run wrk
	wrk -t$WRK_THREADS -c$WRK_CONNECTIONS -d$DURATION http://127.0.0.1:$PORT$url_path

	# Kill the server
	kill $SERVER_PID
	wait $SERVER_PID 2>/dev/null
	echo "--------------------------------------------------"
	echo ""
}

MODE=${1:-hello}

if [ "$MODE" == "hello" ]; then
	echo "RUNNING HELLO WORLD BENCHMARKS"
	run_bench "Moonbeam (Single-Threaded)" "./benchmarks/moonbeam/target/release/st" "/"
	run_bench "Moonbeam (Multi-Threaded, 4 cores)" "./benchmarks/moonbeam/target/release/mt" "/"
	run_bench "Tokio (Multi-Threaded)" "./benchmarks/tokio/target/release/hello" "/"
	run_bench "Rouille (Thread-per-connection)" "./benchmarks/rouille/target/release/rouille-hello" "/"
	run_bench "Node.js (Single-Threaded)" "node benchmarks/nodejs/server.js" "/"
elif [ "$MODE" == "static" ]; then
	echo "RUNNING STATIC FILE BENCHMARKS (4KB)"
	run_bench "Moonbeam (Single-Threaded)" "./benchmarks/moonbeam/target/release/st_static" "/test.txt"
	run_bench "Moonbeam (Multi-Threaded, 4 cores)" "./benchmarks/moonbeam/target/release/mt_static" "/test.txt"
	run_bench "Tokio (Multi-Threaded)" "./benchmarks/tokio/target/release/static" "/test.txt"
	run_bench "Rouille (Thread-per-connection)" "./benchmarks/rouille/target/release/rouille-static" "/test.txt"
	run_bench "Node.js (Single-Threaded)" "node benchmarks/nodejs/static.js" "/test.txt"
else
	echo "Unknown mode: $MODE. Use 'hello' or 'static'."
fi
