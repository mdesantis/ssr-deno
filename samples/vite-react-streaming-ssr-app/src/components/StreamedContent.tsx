import { use } from 'react'

function fetchDelayedMessage(name: string): Promise<string> {
  return new Promise(resolve => {
    setTimeout(() => {
      resolve(`Streamed content for ${name}!`)
    }, 50)
  })
}

export default function StreamedContent({ name }: { name: string }) {
  const message = use(fetchDelayedMessage(name))

  return <p>{message}</p>
}
