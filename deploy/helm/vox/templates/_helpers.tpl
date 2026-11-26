{{/* Names, labels and the image, shared by every template. */}}

{{- define "vox.fullname" -}}
{{- if contains .Chart.Name .Release.Name -}}
{{- .Release.Name | trunc 50 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 50 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "vox.labels" -}}
app.kubernetes.io/name: {{ .Chart.Name }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Values.image.tag | default .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version }}
{{- end -}}

{{/* Selector labels for one component: include "vox.selector" (list . "directory") */}}
{{- define "vox.selector" -}}
{{- $ := index . 0 -}}
app.kubernetes.io/name: {{ $.Chart.Name }}
app.kubernetes.io/instance: {{ $.Release.Name }}
app.kubernetes.io/component: {{ index . 1 }}
{{- end -}}

{{- define "vox.image" -}}
{{ .Values.image.repository }}:{{ .Values.image.tag | default .Chart.AppVersion }}
{{- end -}}

{{- define "vox.secretName" -}}
{{- .Values.secrets.existingSecret | default (printf "%s-secrets" (include "vox.fullname" .)) -}}
{{- end -}}

{{- define "vox.directory" -}}
{{- printf "%s-directory" (include "vox.fullname" .) -}}
{{- end -}}

{{- define "vox.edge" -}}
{{- printf "%s-edge" (include "vox.fullname" .) -}}
{{- end -}}

{{/* A world's resource name: include "vox.world" (list . $world) */}}
{{- define "vox.world" -}}
{{- printf "%s-world-%s" (include "vox.fullname" (index . 0)) (index . 1).id | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{/* The headers the edge sends on with every request (nginx drops the server's own
     proxy_set_header lines in any location that sets one, so each location includes these). */}}
{{- define "vox.edgeHeaders" -}}
proxy_set_header Host $host;
proxy_set_header X-Forwarded-For $client_chain;
proxy_set_header X-Forwarded-Proto $client_proto;
{{- if not .Values.cloudflared.enabled }}
# not behind Cloudflare: nobody gets to say who they are with Cloudflare's header
proxy_set_header CF-Connecting-IP "";
{{- end }}
{{- end -}}
