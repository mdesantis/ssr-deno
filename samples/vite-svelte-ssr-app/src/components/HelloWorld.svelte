<script lang="ts">
  interface HelloWorldProps {
    name: string
  }

  let { name }: HelloWorldProps = $props()
</script>

<div>
  <h2>Hello {name}!</h2>
  <p>I'm a Svelte 5 component rendered on the server.</p>
</div>
