FROM python:3.13-slim-bookworm
WORKDIR /controller
COPY deploy/arr-controller/controller.py ./controller.py
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1
ENTRYPOINT ["python3", "/controller/controller.py"]
