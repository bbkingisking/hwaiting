import { defineConfig } from '@hey-api/openapi-ts'

export default defineConfig({
  input: '../backend/openapi.json',
  output: 'src/lib/api-generated',
  plugins: ['@hey-api/typescript'],
})
