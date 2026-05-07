# frozen_string_literal: true

workers 2
threads 1, 1
preload_app!
log_requests false
quiet

app_path = File.expand_path('config.ru', __dir__)
rackup app_path

on_worker_boot do
  # Bundle.new is lazy inside config_puma.ru — nothing to do here
end
