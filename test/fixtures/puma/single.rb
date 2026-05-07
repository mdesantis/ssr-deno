# frozen_string_literal: true

workers 0
threads 1, 1
log_requests false
quiet

app_path = File.expand_path('config.ru', __dir__)
rackup app_path
