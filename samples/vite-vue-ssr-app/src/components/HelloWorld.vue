<template>
  <div>
    <h2>Hello {{ name }}!</h2>
    <p>I'm a Vue 3 single-file component rendered on the server.</p>
  </div>
</template>

<script setup lang="ts">
interface HelloWorldProps {
  name: string
}

defineProps<HelloWorldProps>()
</script>
