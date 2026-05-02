import {
  Card,
  CardContent,
  Typography,
  CardActions,
  Button,
} from '@mui/material'

interface MuiCardProps {
  title: string
  description: string
}

export default function MuiCard({ title, description }: MuiCardProps) {
  return (
    <Card variant="outlined" sx={{ minWidth: 275 }}>
      <CardContent>
        <Typography variant="h5" component="div" gutterBottom>
          {title}
        </Typography>
        <Typography variant="body2" color="text.secondary">
          {description}
        </Typography>
      </CardContent>
      <CardActions>
        <Button size="small">Learn More</Button>
      </CardActions>
    </Card>
  )
}
