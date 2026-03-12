{{/*
Expand the name of the chart.
*/}}
{{- define "spine.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "spine.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "spine.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "spine.labels" -}}
helm.sh/chart: {{ include "spine.chart" . }}
{{ include "spine.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Core selector labels
*/}}
{{- define "spine.selectorLabels" -}}
app.kubernetes.io/name: {{ include "spine.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Core component labels
*/}}
{{- define "spine.core.labels" -}}
{{ include "spine.labels" . }}
app.kubernetes.io/component: core
{{- end }}

{{/*
Core selector labels
*/}}
{{- define "spine.core.selectorLabels" -}}
{{ include "spine.selectorLabels" . }}
app.kubernetes.io/component: core
{{- end }}

{{/*
Gateway component labels
*/}}
{{- define "spine.gateway.labels" -}}
{{ include "spine.labels" . }}
app.kubernetes.io/component: gateway
{{- end }}

{{/*
Gateway selector labels
*/}}
{{- define "spine.gateway.selectorLabels" -}}
{{ include "spine.selectorLabels" . }}
app.kubernetes.io/component: gateway
{{- end }}

{{/*
Service account name
*/}}
{{- define "spine.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "spine.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Core headless service name (for StatefulSet)
*/}}
{{- define "spine.core.headlessServiceName" -}}
{{- printf "%s-headless" (include "spine.fullname" .) }}
{{- end }}
