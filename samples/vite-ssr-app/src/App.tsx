import HelloWorld from './components/HelloWorld.tsx'

interface AppProps {
  data?: {
    name?: string
  }
  [key: string]: unknown
}

export default function App({ data, ...rest }: AppProps) {
  const name = data?.name ?? 'World'

  return (
    <html>
      <head>
        <title>SSR with Deno - {name}</title>
      </head>
      <body>
        <div id="root">
          <HelloWorld name={name} />
          {Object.keys(rest).length > 0 && (
            <pre style={{ marginTop: '2rem', padding: '1rem', background: '#f5f5f5' }}>
              {JSON.stringify(rest, null, 2)}
            </pre>
          )}
        </div>
      </body>
    </html>
  )
}
