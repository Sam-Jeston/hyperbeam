{{- define "noemune.name" -}}
{{- .Release.Name | trunc 50 | trimSuffix "-" -}}
{{- end -}}

{{- define "noemune.labels" -}}
app.kubernetes.io/name: noemune
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end -}}

{{- define "noemune.selector" -}}
app.kubernetes.io/name: noemune
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: server
{{- end -}}

{{- define "noemune.secretName" -}}
{{- if .Values.existingSecret -}}{{ .Values.existingSecret }}{{- else -}}{{ include "noemune.name" . }}{{- end -}}
{{- end -}}

{{- define "noemune.serviceHost" -}}
{{ include "noemune.name" . }}.{{ .Release.Namespace }}.svc.cluster.local
{{- end -}}
