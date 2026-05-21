# frozen_string_literal: true

module SSR
  module Deno
    class RenderError
      # Extracts the JS error class name from the exception message.
      #
      # Deno formats synchronous throws as "ClassName: message\n    at ..."
      # and async rejections (after err.toString()) as "ClassName: message".
      # Returns nil if the message does not contain a recognisable class prefix.
      def js_error_name
        message.match(/\b(\w+Error):/i) && $1
      end
    end
  end
end
