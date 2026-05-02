<template>
  <div>
    <h1>Vue SSR with Deno</h1>
    <HelloWorld :name="name" />
    <pre v-if="hasData">{{ data }}</pre>
  </div>
</template>

<script setup lang="ts">
import HelloWorld from './components/HelloWorld.vue'

interface AppProps {
  data?: Record<string, unknown>
}

const props = defineProps<AppProps>()
const name = (props.data?.name as string) ?? 'World'
const hasData = props.data && Object.keys(props.data).length > 0
</script>
