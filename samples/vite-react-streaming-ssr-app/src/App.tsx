import { Suspense } from 'react'
import HelloWorld from './components/HelloWorld.tsx'
import StreamedContent from './components/StreamedContent.tsx'

interface AppProps {
  data?: {
    [key: string]: unknown
  }
}

export default function App({ data }: AppProps) {
  const name = (data?.name as string | undefined) ?? 'World'

  return (
    <html>
      <head>
        <meta charSet="utf-8" />
        <title>React Streaming SSR with Deno</title>
      </head>
      <body>
        <div id="root">
          <HelloWorld name={name} />
          <Suspense fallback={<p>Loading streamed content...</p>}>
            <StreamedContent name={name} />
          </Suspense>
        </div>
      </body>
    </html>
  )
}
