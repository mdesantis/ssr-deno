import { h } from 'preact'

interface AppProps {
  data?: {
    name?: string
    [key: string]: unknown
  }
}

export function App({ data }: AppProps) {
  const name = data?.name ?? 'World'

  return h('div', null,
    h('h1', null, 'Preact SSR'),
    h('p', null, `Hello ${name}!`),
    h('p', null, 'This page was server-side rendered with Preact.'),
  )
}
