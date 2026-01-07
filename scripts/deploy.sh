#!/bin/bash

# SPINE Native Deployment Script
set -e

echo "🚀 Starting SPINE Native Deployment..."

# 1. Build the project
echo "🔨 Building SPINE Core..."
cargo build --release -p spine-core

# 2. Create data directories
mkdir -p ./data/seed ./data/node1 ./data/node2

# 3. Start Seed Node
echo "🌱 Starting Seed Node (Port 8080)..."
RUST_LOG=info \
NODE_ID=seed-node \
PORT=8080 \
SPINE_KNOWLEDGE_DIR=./data/seed/knowledge \
SPINE_SESSIONS_DIR=./data/seed/sessions \
./target/release/spine-core > ./data/seed.log 2>&1 &
SEED_PID=$!

sleep 2

# 4. Start Worker Node 1
echo "🐝 Starting Worker Node 1 (Port 8081)..."
RUST_LOG=info \
NODE_ID=node-1 \
PORT=8081 \
SEED_NODES=127.0.0.1:8080 \
SPINE_KNOWLEDGE_DIR=./data/node1/knowledge \
SPINE_SESSIONS_DIR=./data/node1/sessions \
./target/release/spine-core > ./data/node1.log 2>&1 &
NODE1_PID=$!

# 5. Start Worker Node 2
echo "🐝 Starting Worker Node 2 (Port 8082)..."
RUST_LOG=info \
NODE_ID=node-2 \
PORT=8082 \
SEED_NODES=127.0.0.1:8080 \
SPINE_KNOWLEDGE_DIR=./data/node2/knowledge \
SPINE_SESSIONS_DIR=./data/node2/sessions \
./target/release/spine-core > ./data/node2.log 2>&1 &
NODE2_PID=$!

echo "✅ SPINE Cluster Deployed Locally!"
echo "📡 Seed Node PID: $SEED_PID"
echo "📡 Node 1 PID: $NODE1_PID"
echo "📡 Node 2 PID: $NODE2_PID"
echo "📊 Logs are available in ./data/*.log"
echo "🛑 To stop the cluster, run: kill $SEED_PID $NODE1_PID $NODE2_PID"
