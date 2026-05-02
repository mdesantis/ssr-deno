import { renderToString } from 'react-dom/server'
import { App } from './App.tsx'

function render(argsJson: string): string {
  const { data } = JSON.parse(argsJson)
  return renderToString(<App data={data} />)
}

// @ts-ignore: globalThis augmentation
globalThis.render = render
