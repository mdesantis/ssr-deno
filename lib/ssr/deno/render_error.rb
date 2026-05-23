# frozen_string_literal: true

module SSR
  module Deno
    class RenderError < Error
      def js_error_name
        message.match(/\b(\w+Error):/i) && ::Regexp.last_match(1)
      end

      def js_error_message
        # \A\S+ matches Rust error_label ("render", "chunked-render")
        msg = message.sub(/\A\S+ failed to start:\s*/i, '')
        # Deno wraps non-Error sync throws with "Uncaught \"value\""
        msg = msg.sub(/\AUncaught\s+/, '')
        msg = msg.sub(/\A"(.*)"\z/m, '\1')
        msg = msg.sub(/\A\w+Error:\s*/i, '')

        msg.sub(/\n\s+at\s.*\z/m, '')
      end

      def js_error_backtrace
        # NB: false positive possible if error message contains \n    at ...
        m = message.match(/\n((?:\s+at\s.*(?:\n|$))+)/)

        (m && m[1].lines.map(&:strip)) || nil
      end
    end
  end
end
