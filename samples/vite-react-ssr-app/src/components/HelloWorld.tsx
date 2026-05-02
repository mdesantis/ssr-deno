interface HelloWorldProps {
  name: string
}

export default function HelloWorld({ name }: HelloWorldProps) {
  return (
    <div>
      <h1>Hello {name}!</h1>
      <p>I'm an HTML element rendered using Deno!</p>
    </div>
  )
}
