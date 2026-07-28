#!/usr/bin/env bash
set -euo pipefail
student_home=/home/student
rm -rf -- "${student_home}/lab" "${student_home}/answers"
mkdir -p "${student_home}/lab" "${student_home}/answers"
cat > "${student_home}/lab/stream-demo" <<'SCRIPT'
#!/usr/bin/env bash
printf "normal message\n"
printf "error message\n" >&2
SCRIPT
chmod 0755 "${student_home}/lab/stream-demo"
chown -R student:student "${student_home}/lab" "${student_home}/answers"
