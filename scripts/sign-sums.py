"""Signe SHA256SUMS.txt en Ed25519 (signature brute de 64 octets).
Usage : UPDATE_SIGNING_KEY=<graine hex 32 octets> python sign-sums.py <sums> <sortie.sig>"""
import os, sys
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

key = Ed25519PrivateKey.from_private_bytes(bytes.fromhex(os.environ["UPDATE_SIGNING_KEY"].strip()))
open(sys.argv[2], "wb").write(key.sign(open(sys.argv[1], "rb").read()))
