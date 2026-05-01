# SSR Process Pool

> **Source:** Recommendation #4 from [`memory-performance-analysis.md`](memory-performance-analysis.md)
> **Cross-refs:** [`architecture.md`](architecture.md) (isolate pool), [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs) (isolate pool — alternative approach, already implemented), [`memory-performance-analysis.md`](memory-performance-analysis.md) (scaling rules)

---

## Problem

Without the isolate pool, a single V8 isolate per Puma worker caps SSR throughput at `1 / renderToString_time`. The isolate pool (see `IsolatePool` in [`deno_runtime_wrapper.rs`](../ext/ssr_deno/src/deno_runtime_wrapper.rs)) addresses this within a single process, but a process pool remains relevant for use cases requiring failure isolation, multi-host scaling, or even higher throughput.

Additionally, a runaway SSR render (infinite loop, memory leak) can degrade or crash the entire Puma worker, taking down all request handling — not just SSR.

## Approach

Introduce a **dedicated SSR process pool** — a separate set of Ruby processes that run only the ssr-deno runtime, fronted by a lightweight TCP/Unix socket protocol. The Rails app dispatches render requests to the pool and receives HTML responses, keeping SSR failures isolated from the main application.

### Architecture

```
┌─────────────────────────────────────────────────┐
│                  Puma Worker                    │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐       │
│  │ Thread 1 │  │ Thread 2 │  │ Thread 3 │       │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘       │
│       │              │              │           │
│       └──────────────┼──────────────┘           │
│                      │                          │
│              ┌───────▼────────┐                 │
│              │  Pool Client   │                 │
│              │  (connection   │                 │
│              │   pool)        │                 │
│              └───────┬────────┘                 │
└──────────────────────┼──────────────────────────┘
```

### Communication Protocol

Each SSR process listens on a Unix socket (or TCP port). The protocol is a simple request-response over a persistent connection:

1. **Client** connects to socket
2. **Client** sends: `RENDER <bundle_id>\n<json_payload_length>\n<json_payload>`
3. **Server** processes render, sends: `<json_result_length>\n<json_result>`
4. **Connection** stays open for reuse (keep-alive)

**Why a custom protocol instead of HTTP:**

- Lower overhead — no HTTP headers, no parsing
- Unix sockets are faster than TCP loopback
- Simpler to implement — the server is a single-threaded event loop

**Why not use a message queue (Redis, RabbitMQ):**

- Adds infrastructure dependencies
- Higher latency per render (serialization + network hop)
- Overkill for a single-purpose pool

### Pool Client (Ruby — Rails side)

A connection pool (using `connection_pool` gem or custom) that manages connections to SSR processes:

```ruby
module SSR
  module Deno
    class ProcessPool
      # @param socket_paths [Array<String>] List of Unix socket paths
      # @param pool_size [Integer] Max concurrent connections per Puma worker
      # @param timeout [Float] Render timeout per request
      def initialize(socket_paths, pool_size: 5, timeout: 30.0)
        @pool = ConnectionPool.new(size: pool_size, timeout: timeout) do
          connect_to_ssr(socket_paths.sample)
        end
      end

      # @param bundle_id [String]
      # @param args_json [String]
      # @return [String] Rendered HTML (JSON string)
      # @raise [SSR::Deno::RenderError, SSR::Deno::PoolExhaustedError]
      def render(bundle_id, args_json)
        @pool.with do |conn|
          conn.send("RENDER #{bundle_id}\n#{args_json.bytesize}\n#{args_json}")
          result_length = conn.gets.to_i
          result = conn.read(result_length)
          raise SSR::Deno::RenderError, result unless result.start_with?("OK ")
          result[3..] # Strip "OK " prefix
        end
      rescue ConnectionPool::TimeoutError => e
        raise SSR::Deno::PoolExhaustedError, "All SSR pool connections busy: #{e.message}"
      end

      private

      def connect_to_ssr(socket_path)
        socket = UNIXSocket.new(socket_path)
        socket.sync = true
        socket
      end
    end
  end
end
```

### Pool Server (Ruby — standalone process)

A standalone Ruby script that runs the ssr-deno runtime and listens on a Unix socket:

```ruby
#!/usr/bin/env ruby
# bin/ssr-deno-pool

require 'socket'
require 'ssr/deno'

# Load bundles from config
bundles = JSON.parse(ENV.fetch('SSR_BUNDLES', '{}'))
bundles.each { |id, path| SSR::Deno::Bundle.new(path) }

socket_path = ENV.fetch('SSR_SOCKET_PATH', '/tmp/ssr-deno.sock')
server = UNIXServer.new(socket_path)

loop do
  client = server.accept

  Thread.new(client) do |conn|
    begin
      while (line = conn.gets)
        # Parse: RENDER <bundle_id>\n
        cmd, bundle_id = line.strip.split(' ', 2)
        next unless cmd == 'RENDER'

        # Read payload length and payload
        payload_length = conn.gets.to_i
        args_json = conn.read(payload_length)

        # Execute render
        result = SSR::Deno.native_render(bundle_id, args_json)
        conn.puts "OK #{result.bytesize}"
        conn.write(result)
        conn.flush
      end
    rescue SSR::Deno::Error => e
      conn.puts "ERR #{e.message.bytesize}"
      conn.write(e.message)
      conn.flush
    ensure
      conn.close
    end
  end
end
```

### Process Lifecycle Management

For production, the pool processes should be managed by a process supervisor:

| Supervisor | Configuration |
|---|---|
| **systemd** | `ssr-deno-pool@.service` template, one instance per core |
| **foreman** | Add to `Procfile`: `ssrpool: bundle exec ruby bin/ssr-deno-pool` |
| **Kubernetes** | Sidecar container in the same pod as the Rails app |

**Number of processes:** `min(Rails.env.production? ? 4 : 1, Etc.nprocessors)`

### Integration with Rails

In [`railtie.rb`](../lib/ssr/deno/rails/railtie.rb), add a config option to enable the pool:

```ruby
config.ssr_deno.process_pool = {
  enabled: false,           # Default: use embedded V8 (existing behavior)
  socket_paths: [],         # List of Unix socket paths
  pool_size: 5,             # Connections per Puma worker
  timeout: 30.0,            # Per-render timeout
}
```

When `process_pool.enabled` is true, `SSR::Deno::Bundle#render` delegates to the pool client instead of calling `native_render` directly.

### Error Handling

| Scenario | Behavior |
|---|---|
| **SSR process crashes** | Pool client gets connection error, raises `PoolExhaustedError`. Rails helper falls back to CSR (empty string) when `raise_on_render_error` is false. |
| **All pool connections busy** | `ConnectionPool::TimeoutError` → `PoolExhaustedError` → CSR fallback |
| **SSR process hangs** | Socket timeout (configured per-connection) → `PoolExhaustedError` |
| **Bundle not found in pool** | Server returns `ERR BundleNotFoundError` → `SSR::Deno::BundleNotFoundError` on client |

### Performance Projections

| Scenario | Embedded V8 (current) | Process Pool (4 processes) |
|---|---|---|
| Single Puma worker, simple page (10ms render) | ~100 req/s | ~400 req/s |
| Single Puma worker, complex page (40ms render) | ~25 req/s | ~100 req/s |
| 4 Puma workers, simple page | ~400 req/s | ~400 req/s (pool is bottleneck) |
| 4 Puma workers, complex page | ~100 req/s | ~100 req/s (pool is bottleneck) |
| 4 Puma workers + 4 pool processes, simple page | ~400 req/s | ~1,600 req/s |

**Key insight:** The pool helps most when the number of Puma workers is small relative to the number of CPU cores. With many Puma workers, each already has its own V8 isolate, so the pool adds less benefit.

### Memory Impact

Each pool process adds ~20–26 MB (V8 isolate + Deno runtime). For a pool of 4 processes:

| Component | Memory |
|---|---|
| Per pool process | ~20–26 MB |
| 4 pool processes | ~80–104 MB |
| Rails app (no change) | ~100–200 MB per Puma worker |
| **Total additional** | **~80–104 MB** |

---

## Changes

### 1. [`lib/ssr/deno.rb`](../lib/ssr/deno.rb) — Add `PoolExhaustedError`

```ruby
module SSR
  module Deno
    class PoolExhaustedError < Error; end
  end
end
```

### 2. [`lib/ssr/deno/process_pool.rb`](../lib/ssr/deno/process_pool.rb) — New file

Pool client class (as shown above). Depends on the `connection_pool` gem.

### 3. [`lib/ssr/deno/bundle.rb`](../lib/ssr/deno/bundle.rb) — Optional pool delegation

```ruby
def render(data = nil, raw_input: false, raw_output: false)
  reload_if_changed if @auto_reload
  json_input = raw_input ? data : JSON.generate(data)

  instrument 'render.ssr_deno', bundle_name: @bundle_id do
    result = if ProcessPool.enabled?
               ProcessPool.instance.render(@bundle_id, json_input)
             else
               SSR::Deno.native_render(@bundle_id, json_input)
             end
    raw_output ? result : JSON.parse(result)
  end
end
```

### 4. [`lib/ssr/deno/rails/railtie.rb`](../lib/ssr/deno/rails/railtie.rb) — Pool config

Add `process_pool` config option and initialize the pool when enabled.

### 5. [`bin/ssr-deno-pool`](../bin/ssr-deno-pool) — New file

Standalone pool server script (as shown above).

### 6. [`ssr-deno.gemspec`](../ssr-deno.gemspec) — Add `connection_pool` dependency

```ruby
spec.add_dependency 'connection_pool', '~> 2.4'
```

### 7. [`Gemfile`](../Gemfile) — Add `connection_pool` for development

---

## Testing

### Unit test — [`test/ssr/test_process_pool.rb`](../test/ssr/test_process_pool.rb)

```ruby
def test_pool_client_send_render
  # Start a mock SSR server on a Unix socket
  # Send a render request
  # Verify response
end

def test_pool_exhausted_error
  # Exhaust all connections
  # Verify PoolExhaustedError is raised
end

def test_pool_fallback_to_csr
  # Kill the pool server
  # Verify render returns empty string (CSR fallback)
end
```

### Integration test — [`test/ssr/integration_deno_rails.rb`](../test/ssr/integration_deno_rails.rb)

```ruby
def test_process_pool_render
  # Configure pool in dummy app
  # Start pool server
  # Verify ssr_render returns HTML
end
```

### Manual test

```bash
# Terminal 1: Start pool server
SSR_SOCKET_PATH=/tmp/ssr-deno-test.sock bundle exec ruby bin/ssr-deno-pool

# Terminal 2: Send test request
echo -e "RENDER test\n$(echo '{}' | wc -c)\n{}" | nc -U /tmp/ssr-deno-test.sock
```

---

## Implementation Order

1. Add `connection_pool` dependency to gemspec and Gemfile
2. Create `SSR::Deno::PoolExhaustedError` in [`deno.rb`](../lib/ssr/deno.rb)
3. Create [`lib/ssr/deno/process_pool.rb`](../lib/ssr/deno/process_pool.rb) — pool client
4. Create [`bin/ssr-deno-pool`](../bin/ssr-deno-pool) — pool server
5. Modify [`bundle.rb`](../lib/ssr/deno/bundle.rb) — optional pool delegation
6. Modify [`railtie.rb`](../lib/ssr/deno/rails/railtie.rb) — pool config + initialization
7. Add tests
8. Update RBS signatures in [`deno.rbs`](../sig/ssr/deno.rbs)
9. Run `bundle exec rake` to verify full pipeline

---

## Open Questions

1. **Should the pool server use a thread per connection or an event loop (select/poll)?** Thread-per-connection is simpler but doesn't scale to hundreds of concurrent connections. For a pool of 4–8 processes with a small connection pool per Puma worker, thread-per-connection is fine.

2. **Should we support TCP in addition to Unix sockets?** Unix sockets are faster and more secure (filesystem permissions). TCP would be needed if the pool runs on a different host (unlikely for this use case).

3. **Should the pool auto-discover processes via a file-system convention?** e.g., scan `/tmp/ssr-deno-*.sock` on startup. This would simplify configuration.
