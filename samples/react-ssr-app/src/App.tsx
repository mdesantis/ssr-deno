import HelloWorld from './components/HelloWorld.tsx'

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
        <title>SSR with Deno</title>
      </head>
      <body>
        <div id="root">
          <HelloWorld name={name} />
          {Object.keys(data ?? {}).length > 0 && (
            <pre style={{ marginTop: '2rem', padding: '1rem', background: '#f5f5f5' }}>
              {JSON.stringify(data, null, 2)}
            </pre>
          )}
        </div>
      </body>
    </html>
  )
}
