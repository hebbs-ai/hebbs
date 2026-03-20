# ML Inference Service

## Architecture Overview

The ML inference service is a standalone microservice built in Python 3.11 with FastAPI, dedicated to serving real-time machine learning predictions. It runs on GPU-enabled Kubernetes nodes (NVIDIA A100) with autoscaling based on GPU utilization metrics.

The service loads PyTorch models exported in TorchScript format and serves predictions via both REST and gRPC endpoints. Model versioning is managed through MLflow with automated A/B testing for new model deployments.

## Model Pipeline

Training pipelines run on AWS SageMaker with spot instances for cost optimization. Feature engineering uses Apache Spark on EMR for batch processing and Redis for real-time feature serving. The feature store maintains consistency between training and inference environments.

Models are validated against a holdout test set before deployment. Automated canary analysis compares prediction distributions between new and existing models, rolling back automatically if KL divergence exceeds the threshold.

## Performance Characteristics

The service handles 5,000 inference requests per second with p99 latency of 45ms for our primary recommendation model. GPU memory management uses dynamic batching to maximize throughput while meeting latency SLAs. Model warm-up occurs during pod initialization to avoid cold-start latency.

## Monitoring and Alerts

Model performance metrics (accuracy, precision, recall, feature drift) are tracked in a dedicated Grafana dashboard. Data drift detection runs hourly using the Kolmogorov-Smirnov test. Alerts fire when prediction confidence drops below 0.7 for more than 5% of requests over a 30-minute window.
