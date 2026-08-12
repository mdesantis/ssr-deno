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

    // `onShellReady` can fire synchronously — inside this very call to
    // `renderToPipeableStream`, before it returns — whenever the shell has
    // no async dependency of its own (as here: only the Suspense-wrapped
    // child suspends). `pipeFn` may therefore still be unassigned when
    // `onShellReady` runs. `shellReady` + the check right after the call
    // handles both orders: if `onShellReady` fires first, `pipeFn` isn't
    // set yet, so it defers to the post-call check; if the call returns
    // first, `pipeFn` is set before `onShellReady` can run.
    let shellReady = false
    // deno-lint-ignore no-explicit-any
    let pipeFn: ((writable: any) => void) | undefined

    function startPiping() {
      pipeFn!({
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
    }

    const { pipe } = renderToPipeableStream(
      createElement(App, { data: context.data }),
      {
        onShellReady() {
          shellReady = true
          if (pipeFn) startPiping()
        },
        onShellError(err: unknown) {
          reject(err)
        },
      }
    )
    pipeFn = pipe
    if (shellReady) startPiping()
  })
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
