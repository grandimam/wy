import requests


def fetch(url):
    return requests.get(url, timeout=10).json()


def batch(urls):
    return [fetch(url) for url in urls]
