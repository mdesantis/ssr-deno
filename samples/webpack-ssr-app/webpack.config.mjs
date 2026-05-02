import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default {
  target: 'webworker',
  mode: 'production',
  entry: './src/entry-server.ts',
  output: {
    path: path.resolve(__dirname, 'dist/server'),
    filename: 'entry-server.js',
  },
  resolve: {
    extensions: ['.ts', '.js'],
  },
  module: {
    rules: [
      {
        test: /\.ts$/,
        use: 'ts-loader',
        exclude: /node_modules/,
      },
    ],
  },
};
