# Throughput Benchmarks — Bundle vs RactorPool

Date: 2026-05-10
Machine: 24 cores, 60GB RAM, release build.

## Minimal bundle (0.4KB, <0.1ms/render)

Measures overhead — GVL contention and Ractor message-passing dominate.

| Config | req/sec |
|--------|---------|
| Bundle workers=0 threads=1 | 10286 |
| RactorPool workers=0 threads=8 pool=8 | 10014 |
| Bundle workers=16 threads=1 | 55353 |
| RactorPool workers=16 threads=1 pool=1 iso=2 | 46630 |
| Bundle workers=24 threads=1 | **56337** |
| RactorPool workers=24 threads=1 pool=1 iso=2 | 51262 |

```sh
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --no-ractor-pool --workers 0 --threads 1
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --ractor-pool --workers 0 --threads 8 --ractor-pool-size 8 --isolate-pool-size 8
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --no-ractor-pool --workers 16 --threads 1
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --ractor-pool --workers 16 --threads 1 --ractor-pool-size 1 --isolate-pool-size 2
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --no-ractor-pool --workers 24 --threads 1
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --ractor-pool --workers 24 --threads 1 --ractor-pool-size 1 --isolate-pool-size 2
```

Bundle ~10-20% faster. With many Puma workers, each has its own GVL — RactorPool's reply-Ractor overhead per call (~100µs) is pure cost.

## React SSR (453KB, ~5ms/render)

Realistic SSR workload.

| Config | req/sec |
|--------|---------|
| Bundle workers=0 threads=5 | 7693 |
| RactorPool workers=0 threads=5 pool=5 iso=5 | **9884** (1.28x) |
| Bundle workers=16 threads=1 | 42379 |
| RactorPool workers=16 threads=1 pool=1 iso=2 | **43559** (~1x) |

```sh
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-ssr-app/dist/server/entry-server.js --no-ractor-pool --workers 0 --threads 5
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-ssr-app/dist/server/entry-server.js --ractor-pool --workers 0 --threads 5 --ractor-pool-size 5 --isolate-pool-size 5
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-ssr-app/dist/server/entry-server.js --no-ractor-pool --workers 16 --threads 1
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-ssr-app/dist/server/entry-server.js --ractor-pool --workers 16 --threads 1 --ractor-pool-size 1 --isolate-pool-size 2
```

Single-process: RactorPool 28% faster — threads run renders concurrently via GVL release.
Clustered: tied — each worker has own GVL, RactorPool overhead cancels gain.

## MUI Emotion (875KB, 929 modules, node_builtins)

| Config | req/sec |
|--------|---------|
| Bundle workers=0 threads=1 | 1267 |
| RactorPool workers=0 threads=4 pool=4 iso=4 | **1769** (1.4x) |
| Bundle workers=12 threads=1 | **9015** |
| RactorPool workers=12 threads=1 pool=1 iso=2 | 2708 |

```sh
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-mui-emotion-ssr-app/dist/server/entry-server.js --no-ractor-pool --workers 0 --threads 1
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-mui-emotion-ssr-app/dist/server/entry-server.js --ractor-pool --workers 0 --threads 4 --ractor-pool-size 4 --isolate-pool-size 4
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-mui-emotion-ssr-app/dist/server/entry-server.js --no-ractor-pool --workers 12 --threads 1
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-mui-emotion-ssr-app/dist/server/entry-server.js --ractor-pool --workers 12 --threads 1 --ractor-pool-size 1 --isolate-pool-size 2
```

Single-process: RactorPool 1.4x faster.
Clustered: Bundle wins 3.3x — bundle is heavy enough that V8 per-render overhead dominates, and separate GVLs per worker are sufficient.

## MUI Dashboard (3.2MB, 2169 modules, ~80ms/render, node_builtins)

Heaviest bundle. V8 execution time dominates.

| Config | req/sec |
|--------|---------|
| Bundle workers=0 threads=1 | 69 |
| RactorPool workers=0 threads=8 pool=8 iso=8 | **150** (2.2x) |
| Bundle workers=12 threads=1 | **319** |
| RactorPool workers=12 threads=1 pool=1 iso=2 | 310 |

```sh
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-emotion-mui-dashboard-ssr-app/dist/server/entry-server.js --no-ractor-pool --workers 0 --threads 1
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-emotion-mui-dashboard-ssr-app/dist/server/entry-server.js --ractor-pool --workers 0 --threads 8 --ractor-pool-size 8 --isolate-pool-size 8
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-emotion-mui-dashboard-ssr-app/dist/server/entry-server.js --no-ractor-pool --workers 12 --threads 1
ruby scripts/throughput.rb --clients 96 --duration 20 --warmup 5 --bundle samples/vite-react-emotion-mui-dashboard-ssr-app/dist/server/entry-server.js --ractor-pool --workers 12 --threads 1 --ractor-pool-size 1 --isolate-pool-size 2
```

Single-process: RactorPool 2.2x faster — V8 render time dominates, GVL release matters.
Clustered: tied at ~315 req/sec — each worker has own GVL.

## Rules of thumb

| Rule | Detail |
|------|--------|
| RactorPool helps when Puma workers=0, threads≥4 | Single-process multi-thread is where GVL hurts most |
| RactorPool size = Puma threads | More Ractors than concurrent request slots adds overhead |
| More Puma workers help both modes | Each worker = own GVL. Sweet spot ≈ core count |
| Bundle wins in clustered mode | Per-worker GVL eliminates need for RactorPool. Reply-Ractor overhead is pure cost |
| RactorPool overhead per call | Reply-Ractor creation ~100µs. Only pays off when render time >> 100µs AND threads contend on GVL |
| `--isolate-pool-size` | Match expected concurrency: threads × (workers + 1). More isolates than slots wastes memory |
