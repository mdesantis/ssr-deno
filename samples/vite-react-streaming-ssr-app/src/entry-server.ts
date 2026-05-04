import { renderToPipeableStream } from 'react-dom/server'
import { createElement } from 'react'
import App from './App.tsx'

export interface RenderContext {
  data: Record<string, unknown>
}

function render(argsJson: string): Promise<string> {
  const context: RenderContext = typeof argsJson === 'string' ? JSON.parse(argsJson) : argsJson

  return new Promise((resolve, reject) => {
    let html = ''

    const { pipe } = renderToPipeableStream(
      createElement(App, { data: context.data }),
      {
        onShellReady() {
          pipe({
            // deno-lint-ignore no-explicit-any
            on(_event: string | symbol, _listener: (...args: any[]) => void) { return this; },
            write(chunk: Uint8Array | string) {
              if (typeof chunk === 'string') {
                html += chunk
              } else {
                html += new TextDecoder().decode(chunk)
              }
              return true
            },
            end() {
              resolve(html)
              return this
            },
          // deno-lint-ignore no-explicit-any
          } as any)
        },
        onShellError(err: unknown) {
          reject(err)
        },
      }
    )
  })
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
