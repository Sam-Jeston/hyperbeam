{{- define "hyperbeam.name" -}}
{{- .Release.Name | trunc 50 | trimSuffix "-" -}}
{{- end -}}

{{- define "hyperbeam.labels" -}}
app.kubernetes.io/name: hyperbeam
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version }}
{{- end -}}

{{- define "hyperbeam.selector" -}}
app.kubernetes.io/name: hyperbeam
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: server
{{- end -}}

{{- define "hyperbeam.secretName" -}}
{{- if .Values.existingSecret -}}{{ .Values.existingSecret }}{{- else -}}{{ include "hyperbeam.name" . }}{{- end -}}
{{- end -}}

{{- define "hyperbeam.serviceHost" -}}
{{ include "hyperbeam.name" . }}.{{ .Release.Namespace }}.svc.cluster.local
{{- end -}}
