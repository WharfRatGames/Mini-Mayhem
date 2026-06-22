#!/bin/bash
# Deploy hook for *.crumbonium.duckdns.org wildcard cert renewal.
# Install to: /etc/letsencrypt/renewal-hooks/deploy/wildcard-tls.sh
# chmod 755 /etc/letsencrypt/renewal-hooks/deploy/wildcard-tls.sh
#
# Certbot sets RENEWED_LINEAGE to the live dir of the cert that was just renewed.
# Only act on the wildcard cert (crumbonium.duckdns.org-0001).

WILDCARD_LINEAGE="/etc/letsencrypt/live/crumbonium.duckdns.org-0001"

if [ "$RENEWED_LINEAGE" != "$WILDCARD_LINEAGE" ]; then
    exit 0
fi

set -e

NGIRCD_CERT_DIR="/etc/ngircd/certs"
mkdir -p "$NGIRCD_CERT_DIR"

# Copy certs (hook runs as root, so no sudo needed)
cp "$WILDCARD_LINEAGE/fullchain.pem" "$NGIRCD_CERT_DIR/fullchain.pem"
cp "$WILDCARD_LINEAGE/privkey.pem"   "$NGIRCD_CERT_DIR/privkey.pem"

# Restore ACL so thelounge user can read the private key
setfacl -m g:thelounge:r "$NGIRCD_CERT_DIR/privkey.pem" 2>/dev/null || true

# Restart IRC and TheLounge
systemctl restart ngircd
systemctl restart thelounge

echo "wildcard-tls.sh: renewed and restarted ngircd + thelounge"
