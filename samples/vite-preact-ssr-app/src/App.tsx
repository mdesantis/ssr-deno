/** @jsxImportSource react */
interface AppProps {
  data?: {
    name?: string
    [key: string]: unknown
  }
}

export function App({ data }: AppProps) {
  const name = data?.name ?? 'World'

  return (
    <div>
      <h1>Preact SSR</h1>
      <p>Hello {name}!</p>
      <p>This page was server-side rendered with Preact.</p>
    </div>
  )
}
