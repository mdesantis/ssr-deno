# frozen_string_literal: true

require 'test_helper'
require 'socket'
require 'timeout'
require 'tmpdir'

module SSR
  class TestIntegrationPuma < Minitest::Test
    include TestFixturePaths

    def setup
      @tmpdir = Dir.mktmpdir('ssr-deno-puma')
      @socket_single = File.join(@tmpdir, 'single.sock')
      @socket_clustered = File.join(@tmpdir, 'clustered.sock')
      @socket_multi = File.join(@tmpdir, 'multi.sock')
    end

    def teardown
      FileUtils.rm_rf(@tmpdir) if @tmpdir
    end

    def test_single_mode
      require 'puma'
      require 'puma/configuration'

      app = lambda { |_env|
        bundle = SSR::Deno::Bundle.new(MINIMAL_BUNDLE)
        body = bundle.render({})
        [200, { 'content-type' => 'text/html' }, [body]]
      }

      with_puma_single(app, @socket_single) do
        resp = http_get_unix(@socket_single, '/')

        assert_equal '200', resp[:code]
        assert_includes resp[:body], '<h1>world</h1>'
      end
    end

    def test_clustered_on_worker_boot
      puma_dir = File.expand_path('../../test/fixtures/puma', __dir__)
      config_path = File.join(puma_dir, 'clustered_on_worker_boot.rb')
      bundle_gemfile = File.expand_path('../../Gemfile', __dir__)
      pid = nil

      pid = spawn(
        { 'BUNDLE_GEMFILE' => bundle_gemfile },
        'bundle', 'exec', 'puma',
        '-C', config_path,
        '-b', "unix://#{@socket_clustered}",
        chdir: puma_dir,
        out: '/dev/null',
        err: '/dev/null'
      )

      wait_for_socket(@socket_clustered)
      resp = http_get_unix(@socket_clustered, '/')

      assert_equal '200', resp[:code]
      assert_includes resp[:body], '<h1>Puma</h1>'
    ensure
      if pid
        Process.kill(:TERM, pid)
        Process.wait(pid)
      end
    end

    def test_clustered_multi_thread
      puma_dir = File.expand_path('../../test/fixtures/puma', __dir__)
      config_path = File.join(puma_dir, 'clustered_multi_thread.rb')
      bundle_gemfile = File.expand_path('../../Gemfile', __dir__)
      pid = nil

      pid = spawn(
        { 'BUNDLE_GEMFILE' => bundle_gemfile },
        'bundle', 'exec', 'puma',
        '-C', config_path,
        '-b', "unix://#{@socket_multi}",
        chdir: puma_dir,
        out: '/dev/null',
        err: '/dev/null'
      )

      wait_for_socket(@socket_multi)

      results = []
      mutex = Mutex.new
      threads = Array.new(4) do
        Thread.new do
          resp = http_get_unix(@socket_multi, '/')
          mutex.synchronize { results << resp }
        end
      end
      threads.each(&:join)

      assert_equal 4, results.size
      results.each do |r|
        assert_equal '200', r[:code]
        assert_includes r[:body], '<h1>Puma</h1>'
      end
    ensure
      if pid
        Process.kill(:TERM, pid)
        Process.wait(pid)
      end
    end

    private

    def with_puma_single(app, socket_path)
      config = Puma::Configuration.new do |c|
        c.bind "unix://#{socket_path}"
        c.app app
        c.quiet
        c.log_requests false
        c.threads 1, 1
        c.workers 0
      end
      launcher = Puma::Launcher.new(config)
      thr = Thread.new { launcher.run }
      wait_for_socket(socket_path)
      yield
    ensure
      launcher&.stop
      thr&.join(5)
    end

    def http_get_unix(socket_path, path)
      sock = UNIXSocket.new(socket_path)
      sock.write("GET #{path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
      raw = sock.read
      sock.close
      status = raw.lines.first.split[1]
      body = raw.split("\r\n\r\n", 2).last
      { code: status, body: body }
    end

    def wait_for_socket(path, timeout: 30)
      start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      loop do
        UNIXSocket.new(path).close
        return
      rescue Errno::ENOENT, Errno::ECONNREFUSED
        elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - start
        raise "Timeout waiting for socket #{path}" if elapsed > timeout

        sleep 0.1
      end
    end
  end
end
