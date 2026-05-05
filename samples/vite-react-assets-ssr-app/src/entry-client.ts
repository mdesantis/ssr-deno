/// <reference lib="dom" />
import { createElement } from 'react'
import { hydrateRoot } from 'react-dom/client'
import App from './App.tsx'
import './App.css'

type SSRGlobal = typeof globalThis & { __SSR_DATA?: Record<string, unknown> }

hydrateRoot(
  document.getElementById('root')!,
  createElement(App, { data: (globalThis as SSRGlobal).__SSR_DATA })
)
