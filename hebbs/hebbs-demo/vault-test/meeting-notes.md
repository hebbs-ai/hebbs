# Meeting Notes

## 2024-03-10 Sprint Planning

Team decided to prioritize the search feature over the dashboard redesign. Alice will own the search backend. Bob handles the frontend integration. Target: ship by end of March.

## 2024-03-15 Architecture Review

Reviewed the caching strategy. Current Redis setup handles 10K requests per second. Need to add a CDN layer for static assets. Decision: use CloudFront with 24-hour TTL for images and CSS.

## 2024-03-20 Customer Feedback Session

Three customers requested dark mode support. Two customers reported slow search results on mobile. Priority: mobile search performance is P0, dark mode is P1.
