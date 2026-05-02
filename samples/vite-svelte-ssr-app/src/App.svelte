<script lang="ts">
  import HelloWorld from './components/HelloWorld.svelte'

  interface AppProps {
    data?: Record<string, unknown>
  }

  let { data }: AppProps = $props()
  let name = $derived((data?.name as string) ?? 'World')
</script>

<div>
  <h1>Svelte SSR with Deno</h1>
  <HelloWorld {name} />
  {#if data && Object.keys(data).length > 0}
    <pre>{JSON.stringify(data, null, 2)}</pre>
  {/if}
</div>
