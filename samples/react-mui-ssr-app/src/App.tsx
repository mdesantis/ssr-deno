import {
  Container,
  Typography,
  Button,
  Card,
  CardContent,
  Stack,
  Chip,
  Box,
} from '@mui/material'
import FavoriteIcon from '@mui/icons-material/Favorite'

interface AppProps {
  data?: {
    [key: string]: unknown
  }
}

export default function App({ data }: AppProps) {
  const name = (data?.name as string | undefined) ?? 'World'

  return (
    <html>
      <head>
        <meta charSet="utf-8" />
        <title>React MUI Plain SSR</title>
      </head>
      <body>
        <div id="root">
          <Container maxWidth="sm" sx={{ py: 4 }}>
            <Typography variant="h3" component="h1" gutterBottom>
              React MUI Plain SSR
            </Typography>

            <Typography variant="h5" gutterBottom>
              Hello {name}!
            </Typography>

            <Stack direction="row" spacing={2} sx={{ my: 2 }}>
              <Button variant="contained" startIcon={<FavoriteIcon />}>
                Like
              </Button>
              <Button variant="outlined">Share</Button>
              <Chip label="SSR" color="primary" />
            </Stack>

            <Card variant="outlined">
              <CardContent>
                <Typography variant="body1">
                  No explicit CSS extraction — MUI styles are handled by the consuming app.
                </Typography>
              </CardContent>
            </Card>

            {data && Object.keys(data).length > 0 && (
              <Box sx={{ mt: 2 }}>
                <Typography variant="caption" color="text.secondary">
                  Data: {JSON.stringify(data)}
                </Typography>
              </Box>
            )}
          </Container>
        </div>
      </body>
    </html>
  )
}
