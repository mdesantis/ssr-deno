import './App.css'
import logo from './assets/images/logo.svg'
import { createElement } from 'react'

interface AppProps {
  data?: Record<string, unknown>
}

export default function App({ data }: AppProps) {
  const name = (data?.name as string | undefined) ?? 'World'
  const title = (data?.title as string | undefined) || 'React Assets SSR'

  return createElement('div', { className: 'app' },
    createElement('header', { className: 'app-header' },
      createElement('img', { src: logo, alt: 'Logo', className: 'app-logo' }),
      createElement('h1', { className: 'app-title' }, title)
    ),
    createElement('main', { className: 'app-main' },
      createElement('h2', null, `Hello ${name}!`),
      createElement('p', null,
        'This page demonstrates SSR with Vite assets: CSS imports, image imports, and static files.'
      ),
      createElement('div', { className: 'asset-demo' },
        createElement('h3', null, 'Asset Types'),
        createElement('ul', null,
          createElement('li', null, createElement('code', null, 'import "./App.css"'), ' - CSS import (inlined in SSR)'),
          createElement('li', null, createElement('code', null, 'import logo from "./assets/logo.svg"'), ' - Image import (base64 data URI)'),
          createElement('li', null, createElement('code', null, '/robots.txt'), ' - Public file (served from dist/client/)')
        )
      )
    ),
    createElement('footer', { className: 'app-footer' },
      createElement('p', null, 'Server-side rendered with ssr-deno + Vite')
    )
  )
}
