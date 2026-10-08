from concurrent.futures import ThreadPoolExecutor

import requests


def fetch(url):
    return requests.get(url, timeout=10).json()


def batch(urls):
    with ThreadPoolExecutor(max_workers=8) as pool:
        return list(pool.map(fetch, urls))
